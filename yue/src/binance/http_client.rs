use crate::errors::YueError;
use crate::models::{DefaultRateLimiter, HostInfo, RequestInfo};
use governor::{
    Jitter, Quota, RateLimiter,
    clock::DefaultClock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
};
use log::error;
use rand;
use rand::Rng;
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use serde_json::Value;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::{interval, sleep};

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

#[derive(Clone)]
pub struct BinanceRestfulClient {
    client: Arc<Client>,
    max_retries: u16,
}

impl BinanceRestfulClient {
    /// 创建 limiter，立即从 exchangeInfo 获取限额并启动后台刷新任务
    pub async fn new(client: Arc<Client>) -> Arc<Self> {
        Arc::new(Self { client, max_retries: 5 })
    }

    /// 可配置重试次数的构造函数（用于测试）
    pub async fn new_with_retries(client: Arc<Client>, max_retries: u16) -> Arc<Self> {
        Arc::new(Self { client, max_retries })
    }

    ///
    /// # 币安的http调用接口。对于币安的规则。
    ///
    ///  整个流程。
    ///  1. 根据request_info中的host信息，获取令牌。如果超时，则报错。
    ///  2. 判断是否要加上权限，如果有的就加上签名
    ///  3. 发送请求。
    ///  4. 判断是否超时。
    ///
    ///  需要注意的点：
    ///  1. 如果请求http request和获取令牌的话，错误就重试，超过重试，再抛出error
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
        if let Err(e) = compose_security_header(request_info, security.as_ref()) {
            return Err(e);
        }

        // 将最大重试次数转换为usize
        let max_retries = self.max_retries as usize;

        // 尝试多次（包含第一次）
        for attempt in 0..=max_retries {
            // 1. 先获取令牌
            match request_info
                .host
                .acquire_limit_token(request_info.weight, request_info.get_rate_limit_timeout())
                .await
            {
                Ok(_) => { /* got token */ }
                Err(e) => {
                    error!("Acquire token failed: {:?}, attempt {}", e, attempt);
                    if attempt < max_retries {
                        // 随机短等待后重试
                        let wait_ms = retry_wait_ms(RetryWaitKind::AcquireToken, attempt);
                        sleep(Duration::from_millis(wait_ms)).await;
                        continue;
                    } else {
                        return Err(e);
                    }
                }
            }

            // 复制 RequestBuilder 以便重试
            let mut rb = match builder.try_clone() {
                Some(b) => b,
                None => {
                    return Err(YueError::new("无法克隆 RequestBuilder，无法重试"));
                }
            };

            // 设置请求超时
            rb = rb.timeout(Duration::from_millis(request_info.request_timeout_mill_secs as u64));

            // 发送请求
            let send_res = rb.send().await;

            match send_res {
                Err(e) => {
                    error!("HTTP request error: {:?}, attempt {}", e, attempt);
                    if attempt < max_retries {
                        // 退避等待后重试
                        let wait_ms = retry_wait_ms(RetryWaitKind::SendError, attempt);
                        sleep(Duration::from_millis(wait_ms)).await;
                        continue;
                    } else {
                        return Err(YueError::RequestError(e));
                    }
                }
                Ok(resp) => {
                    // 统一处理 418/429：由 rate_limit_wait_ms 决定是否需要等待或返回错误
                    match rate_limit_wait_ms(&resp, request_info.host.clone(), attempt, max_retries).await {
                        Err(e) => return Err(e),
                        Ok(Some(wait_ms)) => {
                            // rate_limit_wait_ms 已安排后台任务放行 host，调用方只需等待并重试
                            sleep(Duration::from_millis(wait_ms)).await;
                            continue;
                        }
                        Ok(None) => { /* 无需等待，继续处理 */ }
                    }
                    return Ok(resp);
                }
            }
        }

        Err(YueError::new("unreachable: exhausted retries"))
    }
}

///
/// ## 接口鉴权逻辑
/// 1. 根据request_info中的has_security来判断是否要启用加密。
/// 2. 如果request_info为true，security为None，则报错。
/// 3. 如果security的security_type为HMAC，则使用tools中的sign_hmac方法处理
/// 4. 如果security的security_type为Ed25519，则使用tools中的sign_ed25519方法处理，通过load_ed25519_signing_key来加载私钥地址
/// 5，把apiKey放在请求的header中的是X-MBX-APIKEY
/// 6. 在queryParam里面加入一个新的signature=签名
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
pub(crate) fn compose_security_header(_request_info: &RequestInfo, security: Option<&BinanceSecurityInfo>) -> Result<(), YueError> {
    if _request_info.has_security && security.is_none() {
        return Err(YueError::new("Request requires security but none provided"));
    }
    Ok(())
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
    use super::BinanceRestfulClient;
    use super::{compose_security_header, rate_limit_wait_ms};
    use crate::errors::YueError;
    use crate::models::{DefaultRateLimiter, HostInfo, RequestInfo};
    use governor::Quota;
    use std::num::NonZeroU32;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::RwLock;
    use tokio::time::sleep;
    use wiremock::{Mock, ResponseTemplate};

    fn create_mock_host_info(host: &str) -> Arc<HostInfo> {
        // 初始 quota（用一个合理默认值，马上会被刷新覆盖）
        let initial_quota = Quota::per_minute(NonZeroU32::new(1000).unwrap()).allow_burst(NonZeroU32::new(300).unwrap());

        let limiter = Arc::new(RwLock::new(Arc::new(DefaultRateLimiter::direct(initial_quota))));
        Arc::new(HostInfo::new(host, 0, limiter))
    }

    /// 测试：compose_security_header 在缺少 security 时返回错误
    ///
    /// 目的：验证鉴权辅助函数在请求需要鉴权但未提供鉴权信息时，能正确返回错误，避免发起未授权请求。
    /// 前置条件：构造一个标记为需要鉴权（has_security = true）的 RequestInfo。
    /// 测试步骤：
    /// 1. 使用 `RequestInfo::from_base_path` 构造一个需要鉴权的 RequestInfo。
    /// 2. 调用 `compose_security_header(&req, None)`（不提供任何 security）。
    /// 断言/期望：
    /// - 函数返回 Err，错误类型为自定义错误（非 panic）。
    /// 边界/异常场景：本用例只验证缺少鉴权的处置，签名正确性留给集成测试覆盖。
    /// 预估耗时：极短（同步函数）。
    #[test]
    fn test_compose_security_header_missing() {
        let host = create_mock_host_info("https://api.test");
        let req = RequestInfo::from_base_path(host, "/", true, 1, Some(1000), Some(1)).unwrap();
        let res = compose_security_header(&req, None);
        assert!(res.is_err());
    }

    /// 测试：rate_limit_wait_ms 在接收到 418 (被封) 时返回 ExchangeRequestError
    ///
    /// 目的：验证当服务端返回 HTTP 418 (Forbidden/封禁) 时，限流处理逻辑能够识别并立即以 ExchangeRequestError 失败，
    ///      并携带必要的 header 信息提示（例如 Retry-After）。
    /// 前置条件：通过 WireMock 模拟一个返回 418 的 HTTP 响应。
    /// 测试步骤：
    /// 1. 启动 WireMock 并配置任意路径返回 418（body 可选）。
    /// 2. 发起请求并获取 Response。
    /// 3. 调用 `rate_limit_wait_ms(&resp, &host, attempt=0, max_retries=0)`。
    /// 断言/期望：
    /// - 返回 Err(YueError::ExchangeRequestError) 且 code == 418。
    /// 边界/异常场景：不验证 body 内容，仅验证返回码与错误类型。
    /// 预估耗时：依赖本地 WireMock，通常小于 100ms。
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
        // Host should have been blocked immediately
        assert!(!host.is_allow_all_request(), "host should be blocked");
        match r {
            Err(YueError::ExchangeRequestError { code, .. }) => assert_eq!(code, 418),
            _ => panic!("expected ExchangeRequestError 418"),
        }
        // allow_all_request is scheduled in background; wait shortly and assert it's allowed
        sleep(Duration::from_millis(20)).await;
        assert!(host.is_allow_all_request(), "host should be allowed after scheduled wait");
    }

    /// 测试：rate_limit_wait_ms 在接收到 429 (限流) 时，根据重试策略返回等待时间或最终错误
    ///
    /// 目的：验证 429 情况下的处理逻辑：
    /// - 当仍有重试次数（attempt < max_retries）时，函数应返回 Some(wait_ms) 以指导调用方等待并重试；
    /// - 当无剩余重试次数时，函数应返回 ExchangeRequestError 表示不可重试的最终失败。
    /// 前置条件：通过 WireMock 模拟返回 429 的 HTTP 响应。
    /// 测试步骤：
    /// 1. 启动 WireMock 并配置返回 429。
    /// 2. 发起请求并获取 Response。
    /// 3. 分别调用 `rate_limit_wait_ms(&resp, &host, attempt=0, max_retries=1)` 和 `rate_limit_wait_ms(&resp2, &host, attempt=0, max_retries=0)`。
    /// 断言/期望：
    /// - 第一个调用返回 Ok(Some(ms))，ms 为正数，表示需要等待；
    /// - 第二个调用返回 Err(YueError::ExchangeRequestError) 且 code == 429。
    /// 边界/异常场景：重试等待由 `retry_wait_ms` 生成，不在本测试中断言具体值，仅断言存在性。
    /// 预估耗时：依赖本地 WireMock，通常小于 100ms（不做真正长等待）。
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
        // host should be blocked immediately
        assert!(!host.is_allow_all_request(), "host should be blocked after limit detected");
        // wait a bit longer than scheduled wait to ensure allow_all_request ran
        sleep(Duration::from_millis(ms1 + 20)).await;
        assert!(host.is_allow_all_request(), "host should be allowed after scheduled wait");

        // 再次获取 response
        let resp2 = client.get(&url).send().await.unwrap();
        // 无重试（attempt >= max_retries） => 返回 Err
        let r2 = rate_limit_wait_ms(&resp2, host.clone(), 0, 0).await;
        assert!(r2.is_err());
        // blocked immediately
        assert!(!host.is_allow_all_request(), "host should be blocked after final limit");
        match r2 {
            Err(YueError::ExchangeRequestError { code, .. }) => assert_eq!(code, 429),
            _ => panic!("expected ExchangeRequestError 429"),
        }
        // scheduled allow
        sleep(Duration::from_millis(20)).await;
        assert!(host.is_allow_all_request(), "host should be allowed after scheduled wait");
    }

    /// 测试：当 HTTP 响应包含 X-MBX-USED-WEIGHT 且接近 host.max_limit 时，触发权重预警并返回等待时间
    ///
    /// 目的：验证 `rate_limit_wait_ms` 能够解析 X-MBX-USED-WEIGHT（优先级：1m -> 5m -> 总体），
    ///      在使用量接近 host 配置的 max_limit 时返回需要等待的毫秒数（提醒调用方降低速率/延后重试）。
    /// 前置条件：
    /// - WireMock 返回 200，并在 header 中加入 x-mbx-used-weight-1m（或5m/默认）的值；
    /// - HostInfo 的 max_limit 设置为一个较大的值以便比较阈值（例如 1000）。
    /// 测试步骤：
    /// 1. 启动 WireMock，返回 200 并附带 header x-mbx-used-weight-1m=960。
    /// 2. 构造 HostInfo 并把 max_limit 设置为 1000；发起请求并获取 Response。
    /// 3. 调用 `rate_limit_wait_ms(&resp, &host, attempt=0, max_retries=1)` 并检查返回值。
    /// 断言/期望：
    /// - 函数返回 Ok(Some(ms))；ms 落在权重等待策略规定的时间区间（当前为 10s..~15s）；
    /// - 当 header 值不接近上限时，应返回 Ok(None)（未在此用例中演示）。
    /// 边界/异常场景：如果 max_limit <= 50，则不会触发等待；如果 header 无法解析为数字则忽略。
    /// 预估耗时：依赖本地 WireMock，通常小于 100ms。
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
        assert!(ms >= 100 && ms < 300, "weight wait should be in short range");
        // host should be blocked
        assert!(!host.is_allow_all_request(), "host should be blocked for weight limit");
        sleep(Duration::from_millis(ms + 50)).await;
        assert!(host.is_allow_all_request(), "host should be allowed after scheduled weight wait");
    }

    /// 测试：在收到 429 时，`request` 在重试耗尽后返回 ExchangeRequestError
    ///
    /// 目的：验证当远端返回 429（Too Many Requests）且客户端配置不重试（max_retries=0）时，
    /// `request` 能正确返回 ExchangeRequestError，并且不在本地进行无限等待。
    /// 前置条件：WireMock 返回 429。
    /// 测试步骤：
    /// 1. 使用 `BinanceRestfulClient::new_with_retries(client, 0)` 创建不重试的客户端；
    /// 2. 发起请求并等待返回；
    /// 断言/期望：
    /// - 返回 Err(YueError::ExchangeRequestError) 且 code == 429；
    /// - Host 在检测到 429 时会被短暂 block（此处不直接断言 block 状态，仅确保最终返回错误）。
    /// 预估耗时：通常小于 100ms。
    #[tokio::test]
    async fn bn_client_429_handling() {
        let mock_server = wiremock::MockServer::start().await;

        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(429).set_body_string("too many"))
            .mount(&mock_server)
            .await;

        let client = Arc::new(reqwest::Client::new());
        // set max_retries=0 so request returns error immediately after one attempt
        let bn = BinanceRestfulClient::new_with_retries(client.clone(), 0).await;

        let host = create_mock_host_info(&mock_server.uri());
        let req_info = RequestInfo::from_base_path(host.clone(), "/test", false, 1, Some(200), Some(1)).unwrap();
        let rb = client.get(req_info.as_ref().as_str());

        let res = bn.request(rb, &req_info, None).await;
        match res {
            Err(YueError::ExchangeRequestError { code, body: _ }) => assert_eq!(code, 429),
            other => panic!("expected ExchangeRequestError 429, got: {:?}", other),
        }
    }

    /// 测试：`BinanceRestfulClient::request` 的成功路径（无鉴权、HTTP 200）
    ///
    /// 目的：验证在正常网络/服务返回 200 的情况下，`request` 能正确走完整个流程并返回 Response：
    /// - 成功获取限流令牌；
    /// - 正确发送 HTTP 请求并接收响应；
    /// - 不触发限流/重试逻辑而是直接返回结果。
    /// 前置条件：WireMock 返回 200 与 body="OK"。
    /// 测试步骤：
    /// 1. 启动 WireMock 并配置任意路径返回 200/OK；
    /// 2. 构造 BinanceRestfulClient 和 RequestInfo（has_security=false，weight=1）；
    /// 3. 构造 RequestBuilder 并调用 `bn.request(rb, &req_info, None)`。
    /// 断言/期望：
    /// - 返回 Ok(Response) 且 status == 200；
    /// - 不发生 panic 或长时间等待。
    /// 边界/异常场景：本用例不验证 header 权重与鉴权逻辑。
    #[tokio::test]
    async fn bn_client_request_success() {
        let mock_server = wiremock::MockServer::start().await;

        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
            .mount(&mock_server)
            .await;

        let client = Arc::new(reqwest::Client::new());
        let bn = BinanceRestfulClient::new(client.clone()).await;

        let host = create_mock_host_info(&mock_server.uri());

        let req_info = RequestInfo::from_base_path(host.clone(), "/test", false, 1, Some(1000), Some(2)).unwrap();

        let rb = client.get(req_info.as_ref().as_str());

        let resp = bn.request(rb, &req_info, None).await.unwrap();
        assert_eq!(resp.status().as_u16(), 200);
    }
}
