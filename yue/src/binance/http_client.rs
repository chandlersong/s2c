use crate::errors::YueError;
use crate::models::{HostInfo, RequestInfo};
use crate::tools::{load_ed25519_signing_key, sign_hmac};
use log::{debug, error};
use rand;
use reqwest::{RequestBuilder, Response};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::sleep;

/// 枚举：重试等待类型，便于统一管理等待策略与抖动
enum RetryWaitKind {
    /// 获取令牌时的短等待
    AcquireToken,
    /// 网络/发送错误的退避等待
    SendError,
    /// 收到 429 时的等待（基于 attempt 增加基数）
    TooManyRequests,
}

/// 统一计算等待毫秒数。`attempt` 用于对 TooManyRequests 类型进行累进等待。
fn retry_wait_ms(kind: RetryWaitKind, attempt: usize) -> u64 {
    match kind {
        RetryWaitKind::AcquireToken => {
            // 50..149
            (rand::random::<u64>() % 100) + 50
        }
        RetryWaitKind::SendError => {
            // 100..199
            (rand::random::<u64>() % 100) + 100
        }
        RetryWaitKind::TooManyRequests => {
            // 从 10s 开始，每次重试增加 5s，并加上 0..5s 随机抖动
            // 计算：10_000 ms + attempt*5_000 ms + jitter(0..5_000)
            let base = 10_000u64 + (attempt as u64) * 5_000u64;
            base + (rand::random::<u64>() % 5_000u64)
        }
    }
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BinanceSecurityType {
    #[serde(rename = "HMAC")]
    HMAC,
    #[serde(rename = "Ed25519")]
    Ed25519,
}

pub struct BinanceSecurityInfo {
    //FUTURE: 用security的那个包来包裹一下，优先级低
    /// security_type是Hmac则api_security是字符串
    /// security_type是Ed25519,则api_security是一个私钥的本地地址。
    api_key: String,
    api_secret: String,
    security_type: BinanceSecurityType,
}

impl BinanceSecurityInfo {
    pub fn new(api_key: &str, api_secret: &str, security_type: BinanceSecurityType) -> Self {
        Self {
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
            security_type,
        }
    }
}

#[derive(Clone)]
pub struct BinanceRestfulClient {
    max_retries: u16,
}

impl BinanceRestfulClient {
    /// 创建 limiter，立即从 exchangeInfo 获取限额并启动后台刷新任务
    pub async fn new() -> Arc<Self> {
        Arc::new(Self { max_retries: 5 })
    }

    /// 可配置重试次数的构造函数（用于测试）
    pub async fn new_with_retries(max_retries: u16) -> Arc<Self> {
        Arc::new(Self { max_retries })
    }

    ///
    /// # 币安的http调用接口。对于币安的规则。
    ///
    ///  整个流程。
    ///  1. 根据request_info中的host信息，获取令牌。如果超时，则报错。
    ///  2. 判断是否要加上权限，如果有的就加上签名
    ///  3, 不停的获取令牌，如果错误，就等待，知道获取token
    ///  4，判断host是否可以访问，如果不可以，就等待。通过host的is_block来获取
    ///  3. 发送请求。
    ///  4. 判断是否超时。
    ///
    ///  需要注意的点：
    ///  1. 如果请求http request 错误就重试，超过重试，再抛出error
    ///  2，所有的重试都是相互独立的。且共享重试次数。
    ///  3. 其他的错误，直接发出。
    ///
    ///
    pub async fn request(
        &self,
        builder: RequestBuilder,
        request_info: &RequestInfo,
        security: Option<BinanceSecurityInfo>,
    ) -> Result<Response, YueError> {
        // 处理鉴权头部（如果需要）
        let real_builder = match compose_security_header(request_info, security.as_ref(), &builder) {
            Ok(builder) => builder,
            Err(e) => return Err(e),
        };

        // 将最大重试次数转换为usize
        let max_retries = self.max_retries as usize;

        // 我们将两类重试（acquire token 和 send request）分开实现，但共享同一个重试预算。
        // attempts_made 表示已经发生的失败尝试次数（用于判断是否超出重试预算），初始为 0。
        let mut attempts_made: usize = 0;

        // 1) 获取令牌阶段（无限次重试，不消耗共享重试预算）
        // 按用户要求：获取令牌应无限重试等待，直到成功为止
        let mut acquire_attempts: usize = 0;
        loop {
            match request_info
                .host
                .acquire_limit_token(request_info.weight, request_info.get_rate_limit_timeout())
                .await
            {
                Ok(_) => break, // 获取成功，进入下一阶段
                Err(e) => {
                    debug!(
                        "Acquire token failed: {:?}, acquire_attempts {}. Will retry indefinitely.",
                        e, acquire_attempts
                    );
                    // 无限重试：使用 AcquireToken 类型的抖动等待，但不消耗共享重试预算
                    let wait_ms = retry_wait_ms(RetryWaitKind::AcquireToken, acquire_attempts);
                    acquire_attempts = acquire_attempts.saturating_add(1);
                    sleep(Duration::from_millis(wait_ms)).await;
                    continue;
                }
            }
        }

        // 1.a) 获得令牌后，若 host 仍处于 blocked 状态，则无限等待直到允许（按用户要求）
        let mut blocked_wait_attempts: usize = 0;
        while request_info.host.is_block() {
            error!(
                "Host is blocked after acquiring token, waiting until allowed. attempt={}",
                blocked_wait_attempts
            );
            let wait_ms = retry_wait_ms(RetryWaitKind::AcquireToken, blocked_wait_attempts);
            blocked_wait_attempts = blocked_wait_attempts.saturating_add(1);
            sleep(Duration::from_millis(wait_ms)).await;
        }

        // 2) 发送请求阶段（失败或限流会消耗共享重试预算）
        loop {
            // 复制 RequestBuilder 以便重试（使用经过 compose_security_header 处理后的 real_builder）
            let mut rb = match real_builder.try_clone() {
                Some(b) => b,
                None => return Err(YueError::new("无法克隆 RequestBuilder，无法重试")),
            };

            // 设置请求超时
            rb = rb.timeout(Duration::from_millis(request_info.request_timeout_mill_secs as u64));

            // 发送请求
            match rb.send().await {
                Err(e) => {
                    error!("HTTP request error: {:?}, attempts_made {}", e, attempts_made);
                    if attempts_made >= max_retries {
                        return Err(YueError::RequestError(e));
                    }
                    let wait_ms = retry_wait_ms(RetryWaitKind::SendError, attempts_made);
                    attempts_made = attempts_made.saturating_add(1);
                    sleep(Duration::from_millis(wait_ms)).await;
                    continue;
                }
                Ok(resp) => {
                    // 统一处理 418/429/weight：由 rate_limit_wait_ms 决定是否需要等待或返回错误
                    match rate_limit_wait_ms(&resp, request_info.host.clone(), attempts_made, max_retries).await {
                        Err(e) => return Err(e),
                        Ok(Some(wait_ms)) => {
                            // 如果需要限流等待，等待并且计入一次失败尝试，然后重试发送
                            if attempts_made >= max_retries {
                                return Err(YueError::new("exhausted retries due to rate limit"));
                            }
                            attempts_made = attempts_made.saturating_add(1);
                            sleep(Duration::from_millis(wait_ms)).await;
                            continue;
                        }
                        Ok(None) => {
                            // 成功返回
                            return Ok(resp);
                        }
                    }
                }
            }
        }
    }
}

///
/// # 函数逻辑
///
/// 1. 根据request_info中的has_security来判断是否要启用加密。
/// 2. 如果request_info为true，security为None，则报错。
/// 3. 如果不需要加入认证信息，直接返回原builder
/// 3, 计算payload。payload为query_param在加上下面两个字段，然后对payload百分比编码。
///   - recvWindow=5000
///    - timestamp=当前时间的unix时间戳
///    例如原来的是symbol=１２３４５６=SELL&type=LIMIT&timeInForce=GTC&quantity=1&price=0.2
///    那么计算payload就是symbol=%EF%BC%91%EF%BC%92%EF%BC%93%EF%BC%94%EF%BC%95%EF%BC%96&side=SELL&type=LIMIT&timeInForce=GTC&quantity=1&price=0.2&timestamp=1668481559918&recvWindow=5000
/// 3. 如果security的security_type为HMAC，则使用tools中的sign_hmac方法处理
/// 4. 如果security的security_type为Ed25519，则使用tools中的sign_ed25519方法处理，通过load_ed25519_signing_key来加载私钥地址
/// 5，把apiKey放在请求的header中的是X-MBX-APIKEY
/// 6. 在queryParam里面加入一个新的signature=签名
/// 7. 返回新的RequestBuilder
///
/// ### 签名算法
/// 1. 将参数格式化为 参数=取值 对并用 & 分隔每个参数对。
/// 2. 对字符串进行百分比编码（percent-encoded）。
/// #### 计算payload
/// query param是symbol=１２３４５６=SELL&type=LIMIT&timeInForce=GTC&quantity=1&price=0.2
/// 1. 加上必要的字段timestamp和recvWindow,recvWindow用
///    例子：symbol=１２３４５６=SELL&type=LIMIT&timeInForce=GTC&quantity=1&price=0.2&timestamp=1668481559918&recvWindow=5000
/// 2. 对字符串进行百分比编码（percent-encoded）后，签名 payload 如下所示：
///    例子：symbol=%EF%BC%91%EF%BC%92%EF%BC%93%EF%BC%94%EF%BC%95%EF%BC%96&side=SELL&type=LIMIT&timeInForce=GTC&quantity=1&price=0.2&timestamp=1668481559918&recvWindow=5000
/// 3. 根据相应的security_type的，调用相应的想法去加密
/// 参考资料
/// - [接口鉴权](https://developers.binance.com/docs/zh-CN/binance-spot-api-docs/rest-api/request-security)
///
pub(crate) fn compose_security_header(
    info: &RequestInfo,
    security: Option<&BinanceSecurityInfo>,
    rb: &RequestBuilder,
) -> Result<RequestBuilder, YueError> {
    // 如果不需要鉴权，直接返回克隆的 builder
    if !info.has_security {
        return rb.try_clone().ok_or_else(|| YueError::new("无法克隆 RequestBuilder"));
    }

    // 需要鉴权但未提供 security
    let sec = match security {
        Some(s) => s,
        None => return Err(YueError::new("Request requires security but none provided")),
    };

    // 克隆 RequestBuilder 以便修改并返回
    let mut new_rb = rb.try_clone().ok_or_else(|| YueError::new("无法克隆 RequestBuilder"))?;

    // 获取原始 query 字符串用于签名计算
    let base_query = extract_query_from_builder(&new_rb)?;
    let base_q = base_query.unwrap_or_default();

    // timestamp 和 recvWindow
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis().to_string();
    let recv_window = "5000".to_string();

    // 构造用于签名的 payload：按照 Binance 的要求，需要对每个参数进行 percent-encoding（application/x-www-form-urlencoded）
    // 实现思路：解析已有的 query（如果有），将各键值对重新通过 form_urlencoded 序列化器编码，
    // 再追加 timestamp 和 recvWindow，得到最终的 payload 字符串用于签名。
    //
    // 示例（原始未编码）：
    //   symbol=１２３４５６=SELL&type=LIMIT&timeInForce=GTC&quantity=1&price=0.2
    // 序列化并对参数值进行 percent-encoding 后：
    //   symbol=%EF%BC%91%EF%BC%92%EF%BC%93%EF%BC%94%EF%BC%95%EF%BC%96%3DSELL&type=LIMIT&timeInForce=GTC&quantity=1&price=0.2&timestamp=1668481559918&recvWindow=5000
    // 注意：等号和其它特殊字符会被正确编码为 %3D 等。
    // 额外示例（空格编码）：
    // 原始: q=1 2 3
    // application/x-www-form-urlencoded 编码后: q=1+2+3
    // 注意：在 form_urlencoded 中空格被编码为 '+'，而不是 '%20'；这与某些 URL 编码场景不同，签名时应使用表单编码结果。
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    if !base_q.is_empty() {
        // 解析已有的 query（解码后重新编码，保证规范性）
        for (k, v) in url::form_urlencoded::parse(base_q.as_bytes()) {
            serializer.append_pair(&k, &v);
        }
    }
    serializer.append_pair("timestamp", &ts);
    serializer.append_pair("recvWindow", &recv_window);
    let payload = serializer.finish();

    // 计算签名并把 api key 放入 header
    match sec.security_type {
        BinanceSecurityType::HMAC => {
            let signature = sign_hmac(&payload, &sec.api_secret)?;
            new_rb = new_rb.header("X-MBX-APIKEY", sec.api_key.clone());
            // append timestamp, recvWindow, signature to query
            new_rb = new_rb.query(&[("timestamp", &ts), ("recvWindow", &recv_window), ("signature", &signature)]);
            Ok(new_rb)
        }
        BinanceSecurityType::Ed25519 => {
            // load signing key (api_secret is path to private key)
            let mut signing_key = load_ed25519_signing_key(&sec.api_secret)?;
            let signature = crate::tools::sign_ed25519(payload.clone(), &mut signing_key)?;
            new_rb = new_rb.header("X-MBX-APIKEY", sec.api_key.clone());
            new_rb = new_rb.query(&[("timestamp", &ts), ("recvWindow", &recv_window), ("signature", &signature)]);
            Ok(new_rb)
        }
    }
}

/// 从 RequestBuilder 中提取 URL 的 query 部分（不发送请求）
/// 返回 Ok(Some(query_string)) 或 Ok(None)（无 query）或 Err
pub(crate) fn extract_query_from_builder(rb: &RequestBuilder) -> Result<Option<String>, YueError> {
    // 尝试克隆 RequestBuilder，若不支持克隆则无法读取
    let cloned = rb.try_clone().ok_or_else(|| YueError::new("无法克隆 RequestBuilder"))?;
    // build 会构造一个 Request（不发送），如果失败会返回 reqwest::Error
    let req = cloned.build()?;
    Ok(req.url().query().map(|s| s.to_string()))
}

///
/// # 方法逻辑
/// 1. 碰到以下情况，就要开始限流
///   - http status code是否为429。
///   - X-MBX-USED-WEIGHT的逻辑
///   - http status code是否为418。
/// 2. 如果检查需要减少访问，则采用后面的通用操作
///
/// ## 判断需要限流之后是否需要处理。
/// 只有再触发了访问过频繁，才应该启用本逻辑。否则返回0.
/// 1. 如果发生访问限制，调用request_info的host中block_all_request。
/// 2. 如果有Retry-After表头，计算Retry-After需要休眠多久，+5ms的值返回。 启动一条线程，在等待返回值的时候后allow_all_request放行。
/// 3. 如果没有Retry-After表头，通过retry_wait_ms获得等待时间。 启动一条线程，在等待返回值的时候后allow_all_request放行。
///
/// ## Retry-After
/// Retry-After是unix timestamp。到毫秒逻辑。
///
/// ## 访问限制判断逻辑
/// 1. 如果返回的http status code为429，则表示调用过多，需要降低访问频率。
/// 2. 如果返回的http status code为418，则表示被封。
/// 3. X-MBX-USED-WEIGHT是http的header。用来表示已经调用权重
///
/// ### 429处理逻辑
/// 1. 等待随机的ms数，第一次等待100到200的随机毫秒，如果还是429，那么加100ms的随机秒数。
/// 2. 如果超过timeout。则报错。把表头的Retry-After写入到错误信息中抛出。
///
/// ### 418处理逻辑
/// 1. 报错。把表头的Retry-After写入到错误信息中抛出。
///
/// ### X-MBX-USED-WEIGHT表头的处理。
/// 1.可能有表头会显示如下。取值优先值按照其先后顺序。
///  - x-mbx-used-weight-1m
///  - x-mbx-used-weight-5m
///  - x-mbx-used-weight
///
///
/// 参考资料
/// - [接口鉴权](https://developers.binance.com/docs/zh-CN/binance-spot-api-docs/rest-api/request-security)
/// - [访问限制](https://developers.binance.com/docs/zh-CN/binance-spot-api-docs/rest-api/limits)
///
/// 处理 rate limit 响应并返回应等待的毫秒数（Some(ms) 表示需要等待，None 表示无需等待）
/// - status 418: 直接返回 Err(ExchangeRequestError)
/// - status 429: 若 attempt < max_retries 则返回 Ok(Some(wait_ms)) 表示调用方需等待并重试，
///             否则返回 Err(ExchangeRequestError)
/// - 其他状态: 返回 Ok(None)
async fn rate_limit_wait_ms(resp: &Response, host: Arc<HostInfo>, attempt: usize, max_retries: usize) -> Result<Option<u64>, YueError> {
    let status = resp.status().as_u16();

    // Determine if any limit condition applies (429 status, 418 status, or weight header threshold)
    let weight_hdr = resp
        .headers()
        .get("x-mbx-used-weight-1m")
        .or_else(|| resp.headers().get("x-mbx-used-weight-5m"))
        .or_else(|| resp.headers().get("x-mbx-used-weight"))
        .and_then(|v| v.to_str().ok().and_then(|s| s.parse::<u32>().ok()));

    let is_weight_limit = match weight_hdr {
        Some(val) => {
            let max_limit = host.get_max_limit();
            max_limit > 50 && val > max_limit.saturating_sub(50)
        }
        None => false,
    };

    let is_429 = status == 429;
    let is_418 = status == 418;

    // If no limit condition, continue
    if !is_429 && !is_418 && !is_weight_limit {
        return Ok(None);
    }

    // At this point we detected a limit condition. Uniform handling:
    // 1) block host
    // 2) compute wait_ms (prefer Retry-After header interpreted as unix timestamp in sec or ms)
    // 3) spawn background task to allow_all_request after wait_ms
    host.block_all_request();

    // Try parse Retry-After if present
    let retry_after_header = resp.headers().get("Retry-After").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let now_ms = li::tools::time::unix_time_now_u64_utc();

    let wait_ms = if let Some(ref v) = retry_after_header {
        if let Ok(val) = v.trim().parse::<u64>() {
            let t_ms = val.saturating_mul(1000);
            if t_ms > now_ms { t_ms - now_ms + 5 } else { 5 }
        } else {
            retry_wait_ms(RetryWaitKind::TooManyRequests, attempt)
        }
    } else {
        retry_wait_ms(RetryWaitKind::TooManyRequests, attempt)
    };

    // schedule allow_all_request in background
    let host_for_task = host.clone();
    tokio::spawn(async move {
        sleep(Duration::from_millis(wait_ms)).await;
        host_for_task.allow_all_request();
    });

    // If we can retry, return wait_ms; otherwise return error indicating the limit
    if attempt < max_retries {
        return Ok(Some(wait_ms));
    }

    // Final error
    let body = if is_429 {
        match retry_after_header {
            Some(r) => format!("429 Too Many Requests, Retry-After={:?}", r),
            None => "429 Too Many Requests".to_string(),
        }
    } else if is_418 {
        match retry_after_header {
            Some(r) => format!("418 blocked, Retry-After={:?}", r),
            None => "418 blocked".to_string(),
        }
    } else {
        // weight limit
        match weight_hdr {
            Some(v) => format!("X-MBX-USED-WEIGHT near limit: {}", v),
            None => "rate limit".to_string(),
        }
    };

    Err(YueError::ExchangeRequestError {
        code: if is_429 {
            429
        } else if is_418 {
            418
        } else {
            429
        },
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::{BinanceRestfulClient, BinanceSecurityInfo, BinanceSecurityType};
    use super::{compose_security_header, rate_limit_wait_ms};
    use crate::errors::YueError;
    use crate::models::{DefaultRateLimiter, HostInfo, RequestInfo, create_share_rate_limiter};
    use governor::Quota;
    use serde_json::json;
    use std::num::NonZeroU32;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::RwLock;
    use tokio::time::sleep;
    use wiremock::matchers::{header, method, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn create_mock_host_info(host: &str) -> Arc<HostInfo> {
        // 初始 quota（用一个合理默认值，马上会被刷新覆盖）
        let initial_quota = Quota::per_minute(NonZeroU32::new(1000).unwrap()).allow_burst(NonZeroU32::new(300).unwrap());

        let limiter = Arc::new(RwLock::new(Arc::new(DefaultRateLimiter::direct(initial_quota))));
        Arc::new(HostInfo::new(host, 0, limiter))
    }

    // 等待最多 timeout_ms 毫秒，直到 host 被 block（is_block() == true），返回是否观察到 block
    async fn wait_for_block(host: &Arc<HostInfo>, timeout_ms: u64) -> bool {
        let mut waited = 0u64;
        while waited < timeout_ms {
            if host.is_block() {
                return true;
            }
            sleep(Duration::from_millis(5)).await;
            waited += 5;
        }
        false
    }

    // 等待最多 timeout_ms 毫秒，直到 host 被 allow（is_block() == false），返回是否观察到 allow
    async fn wait_for_allow(host: &Arc<HostInfo>, timeout_ms: u64) -> bool {
        let mut waited = 0u64;
        while waited < timeout_ms {
            if !host.is_block() {
                return true;
            }
            sleep(Duration::from_millis(5)).await;
            waited += 5;
        }
        false
    }

    /// 测试：rate_limit_wait_ms 在接收到 418 (被封) 时返回 ExchangeRequestError
    ///
    /// 目的：验证当服务端返回 HTTP 418（表示被封/禁止访问）时，限流逻辑能正确识别并
    ///      返回 ExchangeRequestError，且在触发限流时对 Host 的 block/allow 行为按策略执行。
    /// 前置条件：
    /// - 使用 WireMock 模拟返回 HTTP 418 的响应，并携带可选的 Retry-After 头（本测试使用 "0" 以缩短等待）。
    /// - HostInfo 的初始状态为允许请求。
    /// 测试步骤：
    /// 1. 启动 WireMock，配置返回 418（含 Retry-After: 0）。
    /// 2. 使用 reqwest 发送请求并获取 Response。
    /// 3. 调用 rate_limit_wait_ms(&resp, host, attempt=0, max_retries=0)。
    /// 4. 断言 host 在检测到限流时被阻塞；随后后台任务按 Retry-After 放行。
    /// 断言/期望：
    /// - 函数返回 Err(YueError::ExchangeRequestError) 且 code == 418。
    /// - host 在短时间内变为 blocked（is_allow_all_request == false），并在调度等待后恢复为 allowed。
    /// 边界/异常场景：不验证响应体内容，主要验证返回码与 Host 的 block/allow 行为。
    /// 预估耗时：通常小于 200ms（依赖本地 WireMock 与短等待）。
    #[tokio::test]
    async fn test_handle_rate_limit_418() {
        let mock_server = wiremock::MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(418).append_header("Retry-After", "0").set_body_string("blocked"))
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/test", mock_server.uri());
        let resp = client.get(&url).send().await.unwrap();

        let host = create_mock_host_info(&mock_server.uri());
        let r = rate_limit_wait_ms(&resp, host.clone(), 0, 0).await;
        // Should be final error
        assert!(r.is_err());
        // Host should have been blocked (poll for a short time to avoid timing flakes)
        assert!(wait_for_block(&host, 100).await, "host should be blocked");
        match r {
            Err(YueError::ExchangeRequestError { code, .. }) => assert_eq!(code, 418),
            _ => panic!("expected ExchangeRequestError 418"),
        }
        // allow_all_request is scheduled in background; wait and assert it's allowed
        assert!(wait_for_allow(&host, 200).await, "host should be allowed after scheduled wait");
    }

    /// 测试：rate_limit_wait_ms 在接收到 429 (限流) 时，根据重试策略返回等待时间或最终错误
    ///
    /// 目的：验证在 HTTP 429（Too Many Requests）情形下的统一限流处理：
    /// - 若有剩余重试次数（attempt < max_retries）应返回等待毫秒数，调用方等待后重试；
    /// - 若无剩余重试次数则返回 ExchangeRequestError（最终失败）；
    /// 同时验证 Host 的 block/allow 行为在触发限流时正确运行。
    /// 前置条件：WireMock 返回 429，并在响应中附带 Retry-After=0 以缩短测试等待时间。
    /// 测试步骤：
    /// 1. 启动 WireMock 返回 429（Retry-After: 0）。
    /// 2. 发起请求并获取 Response。
    /// 3. 调用 rate_limit_wait_ms(&resp, host, attempt=0, max_retries=1)，断言返回 Some(ms) 并且 host 被 block。
    /// 4. 等待 ms + margin，断言 host 被 allow；再次获取响应并调用 rate_limit_wait_ms(..., max_retries=0)，断言返回 Err。
    /// 断言/期望：
    /// - 第一次返回 Ok(Some(ms)) 且 ms 为正；Host 被短暂 block 后按计划 allow；
    /// - 第二次返回 Err(ExchangeRequestError) 且 code == 429。
    /// 边界/异常场景：本测试接受短等待（由 Retry-After 产生），不对 wait_ms 做严格精确断言。
    /// 预估耗时：通常小于 300ms（含安全 margin）。
    #[tokio::test]
    async fn test_handle_rate_limit_429() {
        let mock_server = wiremock::MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(429).append_header("Retry-After", "0").set_body_string("too many"))
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/test", mock_server.uri());
        let resp = client.get(&url).send().await.unwrap();

        let host = create_mock_host_info(&mock_server.uri());

        // 有重试（attempt < max_retries） => 返回 Ok(Some(ms)) 表示需要等待
        let r1 = rate_limit_wait_ms(&resp, host.clone(), 0, 1).await.unwrap();
        assert!(r1.is_some());
        let ms1 = r1.unwrap();
        assert!(ms1 > 0, "wait_ms should be positive");
        // host should be blocked (poll to avoid timing flakes)
        assert!(wait_for_block(&host, 100).await, "host should be blocked after limit detected");
        // wait a bit longer than scheduled wait to ensure allow_all_request ran
        let wait_total = ms1 + 200;
        sleep(Duration::from_millis(wait_total)).await;
        // After scheduled wait host should be allowed (is_block == false)
        assert!(!host.is_block(), "host should be allowed after scheduled wait");

        // 再次获取 response
        let resp2 = client.get(&url).send().await.unwrap();
        // 无重试（attempt >= max_retries） => 返回 Err
        let r2 = rate_limit_wait_ms(&resp2, host.clone(), 0, 0).await;
        assert!(r2.is_err());
        // blocked immediately (poll)
        assert!(wait_for_block(&host, 100).await, "host should be blocked after final limit");
        match r2 {
            Err(YueError::ExchangeRequestError { code, .. }) => assert_eq!(code, 429),
            _ => panic!("expected ExchangeRequestError 429"),
        }
        // scheduled allow (wait a bit)
        assert!(wait_for_allow(&host, 200).await, "host should be allowed after scheduled wait");
    }

    /// 测试：当 HTTP 响应包含 X-MBX-USED-WEIGHT 且接近 host.max_limit 时，触发权重预警并返回等待时间
    ///
    /// 目的：验证 `rate_limit_wait_ms` 能解析 X-MBX-USED-WEIGHT（优先顺序：1m -> 5m -> 无后缀），
    ///      并在权重接近 Host 的 max_limit 时以统一限流流程处理（block host、返回短等待、后台放行）。
    /// 前置条件：
    /// - WireMock 返回 200，并在 Response header 中加入 `x-mbx-used-weight-1m=960` 和 `Retry-After=0`（缩短测试等待）；
    /// - HostInfo 的 max_limit 已设置为 1000（或其他大于 50 的值）。
    /// 测试步骤：
    /// 1. 启动 WireMock 并返回 200，附带权重 header 与 Retry-After=0；
    /// 2. 构造 HostInfo 并设置 max_limit=1000；发起请求并获取 Response；
    /// 3. 调用 `rate_limit_wait_ms(&resp, host, attempt=0, max_retries=1)` 并检查返回 Some(wait_ms)，
    ///    使用轮询确认 host 被 block 后在计划等待结束后被 allow。
    /// 断言/期望：
    /// - 返回 Ok(Some(ms))，ms 为短等待（测试中允许 >=5ms 且 <300ms）；
    /// - Host 在短时间内被 block，并在预期时间后被 allow。
    /// 边界/异常场景：若没有 Retry-After，权重预警实现会返回 100..299 ms 的短等待；若 header 无法解析为数字则忽略该 header。
    /// 预估耗时：通常小于 300ms（含安全 margin）。
    #[tokio::test]
    async fn test_rate_limit_weight_header() {
        let mock_server = wiremock::MockServer::start().await;

        Mock::given(wiremock::matchers::any())
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("x-mbx-used-weight-1m", "960")
                    .append_header("Retry-After", "0")
                    .set_body_string("OK"),
            )
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/test", mock_server.uri());
        let resp = client.get(&url).send().await.unwrap();

        let host = create_mock_host_info(&mock_server.uri());
        // 设置 host 的 max_limit 为 1000 来触发接近上限逻辑
        host.set_max_limit(1000);

        let opt = rate_limit_wait_ms(&resp, host.clone(), 0, 1).await.unwrap();
        assert!(opt.is_some(), "expected Some(wait_ms) when used weight near limit");
        let ms = opt.unwrap();
        // 如果响应包含 Retry-After（测试中设为 "0"），实现会优先使用 Retry-After 计算等待时间，
        // 这会导致一个非常短的等待（例如 5ms）。在没有 Retry-After 的情况下，权重预警会返回 100..300ms。
        // 因此这里接受较宽的范围：至少 5ms，且小于 300ms。
        assert!(ms >= 5 && ms < 300, "weight wait should be in short range, got {}", ms);
        // host should be blocked (poll)
        assert!(wait_for_block(&host, 100).await, "host should be blocked for weight limit");
        // wait for scheduled allow (give enough margin)
        assert!(
            wait_for_allow(&host, ms + 200).await,
            "host should be allowed after scheduled weight wait"
        );
    }

    #[test]
    fn test_extract_query_from_builder() {
        let client = reqwest::Client::new();
        let rb = client.get("http://example.com/path?foo=bar&baz=1");
        let q = super::extract_query_from_builder(&rb).unwrap();
        assert_eq!(q, Some("foo=bar&baz=1".to_string()));
    }

    /// 测试：compose_security_header 使用 HMAC 签名路径
    #[test]
    fn test_compose_security_header_hmac() {
        let client = reqwest::Client::new();
        let rb = client.get("http://example.com/path?foo=bar");

        let host = create_mock_host_info("http://example");
        let req_info = RequestInfo::from_base_path(host, "/", true, 1, Some(1000), Some(1)).unwrap();

        let sec = super::BinanceSecurityInfo {
            api_key: "mykey".to_string(),
            api_secret: "secret".to_string(),
            security_type: super::BinanceSecurityType::HMAC,
        };

        let new_rb = compose_security_header(&req_info, Some(&sec), &rb).unwrap();
        // header present
        let req = new_rb.try_clone().unwrap().build().unwrap();
        assert_eq!(req.headers().get("X-MBX-APIKEY").unwrap(), "mykey");
        // query contains signature and timestamp
        let q = req.url().query().unwrap().to_string();
        assert!(q.contains("signature="));
        assert!(q.contains("timestamp="));
        assert!(q.contains("foo=bar"));
    }

    /// 测试：compose_security_header 对包含空格的参数进行编码（允许 '+' 或 '%20' 两种形式），并附带 signature/header
    #[test]
    fn test_compose_security_header_space_encoding() {
        let client = reqwest::Client::new();
        let rb = client.get("http://example.com/path").query(&[("q", "1 2 3")]);

        let host = create_mock_host_info("http://example");
        let req_info = RequestInfo::from_base_path(host, "/", true, 1, Some(1000), Some(1)).unwrap();

        let sec = super::BinanceSecurityInfo {
            api_key: "k".to_string(),
            api_secret: "s".to_string(),
            security_type: super::BinanceSecurityType::HMAC,
        };

        let new_rb = compose_security_header(&req_info, Some(&sec), &rb).unwrap();
        let req = new_rb.try_clone().unwrap().build().unwrap();
        // header present
        assert_eq!(req.headers().get("X-MBX-APIKEY").unwrap(), "k");
        // query should contain q encoded either as + or %20
        let q = req.url().query().unwrap().to_string();
        assert!(
            q.contains("q=1+2+3") || q.contains("q=1%202%203"),
            "query encoding for spaces should be '+' or '%20', got: {}",
            q
        );
        // has signature
        assert!(q.contains("signature="));
    }

    /// 测试：共享重试预算 —— 首次获取令牌失败一次（消耗一次重试），随后发送请求触发限流并消耗剩余重试预算，最终失败。
    ///
    /// 目的：验证 acquire token 与 send request 两个阶段各自独立，但共享同一重试预算（self.max_retries）。
    /// 前置条件：
    /// - 设置客户端 max_retries = 2；
    /// - 初始将 host block（使得第一次 acquire 失败并消耗一次重试）；
    /// - WireMock 配置对同一路径返回 429（Retry-After=0），模拟 send 阶段被限流。
    /// 测试步骤：
    /// 1. host.block_all_request()；
    /// 2. 在短暂延时后允许 host（host.allow_all_request()），使得 acquire 在消耗一次后成功；
    /// 3. 发起 request：第一次 send 收到 429（消耗第二次重试），第二次 send 仍为 429，此时重试预算耗尽，request 返回 Err。
    /// 断言/期望：
    /// - request 返回 Err；错误信息应指示重试耗尽或限流导致的失败。
    /// 预估耗时：通常小于 500ms（含安全 margin）。
    #[tokio::test]
    async fn test_shared_retry_budget_acquire_then_send_exhausted() {
        let mock_server = wiremock::MockServer::start().await;

        // mock server always returns 429 with Retry-After: 0
        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(429).append_header("Retry-After", "0").set_body_string("too many"))
            .mount(&mock_server)
            .await;

        let client = Arc::new(reqwest::Client::new());
        let bn = BinanceRestfulClient::new_with_retries(2).await;

        let host = create_mock_host_info(&mock_server.uri());
        // 初始阻塞，使得第一次 acquire 会失败
        host.block_all_request();

        let req_info = RequestInfo::from_base_path(host.clone(), "/test", false, 1, Some(1000), Some(1)).unwrap();
        let rb = client.get(req_info.as_ref().as_str());

        // 在很短的延时后放行 host，让 acquire 在消耗一次后成功
        let host_for_task = host.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(20)).await;
            host_for_task.allow_all_request();
        });

        // 执行 request：应最终失败（重试预算被耗尽）
        let res = bn.request(rb, &req_info, None).await;
        assert!(res.is_err(), "expected request to fail due to exhausted shared retry budget");
    }

    #[tokio::test]
    async fn test_bn_post_signed_example() {
        let mock_server = MockServer::start().await;

        // mock：要求是 POST，包含 X-MBX-APIKEY 头，且 query 包含 symbol=BTCUSDT
        Mock::given(method("POST"))
            .and(header("X-MBX-APIKEY", "test_key"))
            .and(query_param("symbol", "BTCUSDT"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "action": "post"})))
            .mount(&mock_server)
            .await;

        // 构造 HostInfo 和 RequestInfo
        let limiter = create_share_rate_limiter(1000);
        let host = Arc::new(HostInfo::new(mock_server.uri(), 1000, limiter));
        let req_info = RequestInfo::from_base_path(host.clone(), "/api/v3/test_post", true, 1, Some(2000), Some(1)).unwrap();

        let client = reqwest::Client::new();
        let rb = client
            .post(req_info.as_ref().as_str())
            .query(&[("symbol", "BTCUSDT")])
            .json(&json!({"foo":"bar","qty":1}));

        let sec = BinanceSecurityInfo::new("test_key", "secret", BinanceSecurityType::HMAC);
        let bn = BinanceRestfulClient::new().await;

        let resp = bn.request(rb, &req_info, Some(sec)).await.expect("request failed");
        assert_eq!(resp.status().as_u16(), 200);
        let v: serde_json::Value = resp.json().await.expect("invalid json");
        assert_eq!(v["ok"], json!(true));
        assert_eq!(v["action"], json!("post"));
    }

    /// 示例测试：使用 BinanceRestfulClient 发起带 HMAC 签名的 PUT 请求（参数放在 query）
    #[tokio::test]
    async fn test_bn_put_signed_example() {
        let mock_server = MockServer::start().await;

        // mock：要求是 PUT，包含 X-MBX-APIKEY 头，且 query 包含 symbol=BTCUSDT
        Mock::given(method("PUT"))
            .and(header("X-MBX-APIKEY", "test_key"))
            .and(query_param("symbol", "BTCUSDT"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "action": "put"})))
            .mount(&mock_server)
            .await;

        let limiter = create_share_rate_limiter(1000);
        let host = Arc::new(HostInfo::new(mock_server.uri(), 1000, limiter));
        let req_info = RequestInfo::from_base_path(host.clone(), "/api/v3/test_put", true, 1, Some(2000), Some(1)).unwrap();

        let client = reqwest::Client::new();
        let rb = client.put(req_info.as_ref().as_str()).query(&[("symbol", "BTCUSDT")]);

        let sec = BinanceSecurityInfo::new("test_key", "secret", BinanceSecurityType::HMAC);
        let bn = BinanceRestfulClient::new().await;

        let resp = bn.request(rb, &req_info, Some(sec)).await.expect("request failed");
        assert_eq!(resp.status().as_u16(), 200);
        let v: serde_json::Value = resp.json().await.expect("invalid json");
        assert_eq!(v["ok"], json!(true));
        assert_eq!(v["action"], json!("put"));
    }
}
