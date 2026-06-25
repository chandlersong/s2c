use crate::errors::YueError;
use crate::models::RequestInfo;
use governor::Jitter;
use log::error;
use reqwest::{Client, RequestBuilder, Response};
use serde::de::DeserializeOwned;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::time::sleep;

pub(crate) static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

pub fn init_http_client(proxy: Option<&str>) -> &'static Client {
    HTTP_CLIENT.get_or_init(|| {
        let mut res = Client::builder();
        if let Some(proxy_url) = proxy {
            res = res.proxy(reqwest::Proxy::all(proxy_url).unwrap());
        } else {
            res = res.no_proxy(); // ��确禁用所有代理,否则他可能走系统代理
        }
        res.build().unwrap()
    })
}

pub fn get_http_client() -> &'static Client {
    HTTP_CLIENT.get().expect("HTTP客户端未初始化，请先调用 init_http_client()")
}

pub async fn execute_public_json_request<U>(info: &RequestInfo, request_builder: RequestBuilder) -> Result<U, YueError>
where
    U: DeserializeOwned + Send + Sync,
{
    let client = CommonPublicRestfulClient::new().await;
    let response = client.request(request_builder, info).await?;
    // Read raw bytes first and then deserialize with serde_json so that
    // JSON parse errors are returned as serde_json::Error (mapped to YueError::SerdeError)
    // instead of being wrapped only inside reqwest::Error.
    let bytes = response.bytes().await?;
    // 尝试反序列化；如果失败，则打印响应 body 以便调试，并返回 serde 错误
    match serde_json::from_slice::<U>(&bytes) {
        Ok(res) => Ok(res),
        Err(e) => {
            // 打印到 stderr，避免调试信息混入正常输出
            error!(
                "execute_json_request - failed to parse JSON, response body: {}",
                String::from_utf8_lossy(&bytes)
            );
            Err(e.into())
        }
    }
}

#[derive(Clone)]
pub struct CommonPublicRestfulClient {
    max_retries: u16,
}

impl CommonPublicRestfulClient {
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
    ///  5. 发送请求。
    ///  6. 判断请求是否触发等待超时。
    ///
    ///  需要注意的点：
    ///  1. 如果请求http request 错误就重试，超过重试，再抛出error
    ///  2，所有的重试都是相互独立的。
    ///  3. 其他的错误，直接发出。
    ///  4. 所有等待的点，都不能超过request_info中的request_timeout_mill_secs
    ///
    ///
    pub async fn request(&self, builder: RequestBuilder, request_info: &RequestInfo) -> Result<Response, YueError> {
        // 将最大重试次数转换为usize
        let max_retries = self.max_retries as usize;

        // 我们将两类重试（acquire token 和 send request）分开实现，但共享同一个重试预算。
        // attempts_made 表示已经发生的失败尝试次数（用于判断是否超出重试预算），初始为 0。
        let mut attempts_made: usize = 0;

        // 用于确保所有等待（包括 acquire/blocked/send/rate-limit 等）的总计不超过
        // request_info.request_timeout_mill_secs。改为记录 start_time 并在每次需要时
        // 基于真实经过时间计算 elapsed_ms（避免依赖 sleep 的累加误差）。
        let total_timeout_ms = request_info.request_timeout_mill_secs;
        let start_time = Instant::now();
        // 累积需要从总耗时中剔除的时长（毫秒），目前仅用于剔除 acquire_limit_token 的耗时
        let mut excluded_acquire_ms: u64 = 0;

        // helper: we will inline remaining/to_sleep calculation at each sleep point to avoid
        // closure borrow issues (we need to mutate cumulative_waited_ms after await).

        // 1) 获取令牌阶段（无限次重试，不消耗共享重试预算）
        // 按用户要求：获取令牌应无限重试等待，直到成功为止。但现在加入总等待时长限制
        // 1.a) 获得令牌后，若 host 仍处于 blocked 状态，则无限等待直到允许（按用户要求），但受总等待时长限制

        // 2) 发送请求阶段（失败或限流会消耗共享重试预算）
        loop {
            //估计应该出现这种情况的时候应该是最后，所以等个5s左右，随机散开
            if request_info.host.check_slow_down().await {
                let jitter = Jitter::up_to(Duration::from_secs(5));
                tokio::time::sleep(jitter + Duration::ZERO).await;
            }
            // 在调用 acquire_limit_token 前记录时间，获取后把该耗时从总计等待时间中剔除。
            let t_acquire_start = Instant::now();

            match request_info
                .host
                .acquire_limit_token(request_info.weight, request_info.request_timeout_mill_secs)
                .await
            {
                Ok(snapshot) => snapshot,
                Err(e) => match e {
                    // 如果是超时错误，直接返回该错误；否则仅重试（continue）
                    YueError::Timeout(_) => return Err(e),
                    _ => {
                        // 仅在非超时错误的情况下继续重试；在这类失败上我们也应该把这次 acquire 的耗时计入剔除
                        let delta_ms = t_acquire_start.elapsed().as_millis() as u64;
                        excluded_acquire_ms = excluded_acquire_ms.saturating_add(delta_ms);
                        continue;
                    }
                },
            };

            // 计算此次 acquire 的耗时并从总耗时中剔除
            let delta_ms = t_acquire_start.elapsed().as_millis() as u64;
            excluded_acquire_ms = excluded_acquire_ms.saturating_add(delta_ms);

            // 计算有效已耗时（排除 acquire 的耗时），用于后续剩余时间计算
            let elapsed_total_ms = start_time.elapsed().as_millis() as u64;
            let elapsed_effective_ms = if elapsed_total_ms > excluded_acquire_ms {
                elapsed_total_ms - excluded_acquire_ms
            } else {
                0
            };

            // 复制 RequestBuilder 以便重试（使用经过 compose_security_header 处理后的 real_builder）
            let mut rb = match builder.try_clone() {
                Some(b) => b,
                None => return Err(YueError::new("无法克隆 RequestBuilder，无法重试")),
            };

            // 为本次 HTTP 请求设置超时：使用总超时减去已有效耗时（排除 acquire 的耗时）
            let remaining_for_send = total_timeout_ms.saturating_sub(elapsed_effective_ms);
            if remaining_for_send == 0 {
                return Err(YueError::new("total wait time exceeded request timeout before sending request"));
            }
            rb = rb.timeout(Duration::from_millis(remaining_for_send));

            // 发送请求
            match rb.send().await {
                Err(e) => {
                    error!("HTTP request error: {:?}, attempts_made {}", e, attempts_made);
                    if attempts_made >= max_retries {
                        return Err(YueError::from(e));
                    }
                    let interval = 100 * attempts_made;
                    let wait_ms =
                        (Jitter::new(Duration::from_millis(100), Duration::from_millis(interval as u64)) + Duration::ZERO).as_millis() as u64;

                    // decide sleep respecting total timeout (recompute elapsed before sleep, exclude acquire time)
                    let elapsed_total_ms = start_time.elapsed().as_millis() as u64;
                    let elapsed_effective_ms = if elapsed_total_ms > excluded_acquire_ms {
                        elapsed_total_ms - excluded_acquire_ms
                    } else {
                        0
                    };
                    let remaining = total_timeout_ms.saturating_sub(elapsed_effective_ms);
                    if remaining == 0 {
                        return Err(YueError::new("total wait time exceeded request timeout"));
                    }
                    let to_sleep = if wait_ms >= remaining { remaining } else { wait_ms };
                    attempts_made = attempts_made.saturating_add(1);
                    sleep(Duration::from_millis(to_sleep)).await;

                    // 重新计算真实经过时间并判断是否超时（剔除 acquire 的耗时）
                    let elapsed_total_ms = start_time.elapsed().as_millis() as u64;
                    let elapsed_effective_ms = if elapsed_total_ms > excluded_acquire_ms {
                        elapsed_total_ms - excluded_acquire_ms
                    } else {
                        0
                    };
                    if elapsed_effective_ms >= total_timeout_ms {
                        return Err(YueError::new("total wait time exceeded request timeout after send error"));
                    }
                    continue;
                }
                Ok(resp) => {
                    return Ok(resp);
                }
            }
        }
    }
}
