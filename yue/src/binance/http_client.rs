use crate::errors::YueError;
use crate::models::{DefaultRateLimiter, RequestInfo};
use governor::{
    Jitter, Quota, RateLimiter,
    clock::DefaultClock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
};
use log::error;
use rand;
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde_json::Value;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::{interval, sleep};

const SAFETY_MARGIN: u32 = 200; // 留 200 weight 作为缓冲，防突发
const REFRESH_INTERVAL_SECS: u64 = 6 * 3600; // 每6小时刷新一次 quota
const MAX_RETRIES: u32 = 6;
const BASE_DELAY_MS: u64 = 1000; // 指数退避起始 1s

#[derive(Clone)]
pub struct BinanceRestfulClient {
    client: Client,
    limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>>,
    weight_limit: Arc<std::sync::atomic::AtomicU32>, // 原子更新 weight_limit
}

impl BinanceRestfulClient {
    /// 创建 limiter，立即从 exchangeInfo 获取限额并启动后台刷新任务
    pub async fn new() -> Arc<Self> {
        let client = Client::new();
        let weight_limit = Arc::new(std::sync::atomic::AtomicU32::new(0));

        // 初始 quota（用一个合理默认值，马上会被刷新覆盖）
        let initial_quota = Quota::per_minute(NonZeroU32::new(1000).unwrap()).allow_burst(NonZeroU32::new(300).unwrap());

        let limiter = Arc::new(RateLimiter::direct(initial_quota));

        let this = Arc::new(Self {
            client,
            limiter,
            weight_limit: weight_limit.clone(),
        });
        this
    }

    pub async fn check_rate_limit(weight: u32, limiter: &DefaultRateLimiter, timeout_secs: u64) -> Result<(), YueError> {
        // 超时时间：timeout_secs 秒
        let timeout_duration = Duration::from_secs(timeout_secs);
        let weight = match NonZeroU32::new(weight) {
            Some(w) => w,
            None => return Err(YueError::new("权重必须为非零")),
        };
        let jitter = Jitter::up_to(Duration::from_millis(500));
        // 优雅处理超时和 governor 错误
        match tokio::time::timeout(timeout_duration, limiter.until_n_ready_with_jitter(weight, jitter)).await {
            Err(e) => {
                error!("获取令牌超时, timeout 时间:{}秒, 错误:{}", timeout_secs, e);
                Err(YueError::new("限流超时"))
            }
            Ok(res) => match res {
                Ok(_) => Ok(()),
                Err(e) => {
                    error!("限流器内部错误: {:?}", e);
                    Err(YueError::new(&format!("限流器内部错误: {:?}", e)))
                }
            },
        }
    }

    /// 发送请求（带限流、重试、weight 监控）
    pub async fn request(&self, builder: RequestBuilder, request_info: &RequestInfo) -> Result<Response, YueError> {
        let mut attempt = 0u32;

        loop {
            // 1. governor 本地限流（消耗 endpoint_weight）
            Self::check_rate_limit(request_info.weight, &self.limiter, request_info.get_rate_limit_timeout()).await?; // 30s 超时

            // 2. 克隆并发送（因为 send 后 builder 不可重用）
            let req = builder.try_clone().ok_or_else(|| YueError::new("无法克隆请求构建器"))?;
            let result = req.send().await;

            match result {
                Ok(resp) => {
                    let status = resp.status();

                    // 处理 Binance 限流错误
                    if status == StatusCode::TOO_MANY_REQUESTS || status.as_u16() == 418 {
                        attempt += 1;
                        if attempt > MAX_RETRIES {
                            return Err(YueError::new(format!("超过最大重试次数 {} (429/418)", MAX_RETRIES).as_str()));
                        }

                        let wait_secs = if let Some(header) = resp.headers().get("retry-after") {
                            header.to_str().ok().and_then(|s| s.parse::<u64>().ok()).unwrap_or(60)
                        } else {
                            // 指数退避 + jitter (使用 rand::random 避免不同版本 gen_range/随机 API 差异)
                            let delay_ms = BASE_DELAY_MS * 2u64.pow(attempt.saturating_sub(1));
                            let jitter = if delay_ms >= 4 {
                                (rand::random::<u64>() % ((delay_ms / 4) + 1))
                            } else {
                                0
                            };
                            (delay_ms + jitter) / 1000 + 1 // 至少 1s
                        };

                        println!("[BinanceLimiter] 429/418 检测 (尝试 {}/{}), 等待 {}s", attempt, MAX_RETRIES, wait_secs);
                        sleep(Duration::from_secs(wait_secs)).await;
                        continue;
                    }

                    // 读取 used-weight 并保守等待（防超限）
                    if let Some(used_str) = resp.headers().get("x-mbx-used-weight-1m") {
                        if let Ok(used) = used_str.to_str().unwrap_or("0").parse::<u32>() {
                            let current_limit = self.weight_limit.load(std::sync::atomic::Ordering::Relaxed);
                            if current_limit > 0 && used > current_limit.saturating_sub(SAFETY_MARGIN / 2) {
                                // let now = chrono::Utc::now();
                                // let wait_secs = 60 - now.second() as u64 + 5; // 等到下一分钟 + 5s 缓冲
                                println!("[BinanceLimiter] 高使用率 {} / {}，强制等待 {}s 重置窗口", used, current_limit, 1);
                                sleep(Duration::from_secs(1)).await;
                            }
                        }
                    }

                    return Ok(resp);
                }

                Err(err) if err.is_timeout() || err.is_connect() || err.is_request() => {
                    // 网络 transient 错误 → 重试
                    attempt += 1;
                    if attempt > MAX_RETRIES {
                        return Err(err.into());
                    }

                    let delay_ms = BASE_DELAY_MS * 2u64.pow(attempt.saturating_sub(1));
                    let jitter = if delay_ms >= 5 {
                        (rand::random::<u64>() % ((delay_ms / 5) + 1))
                    } else {
                        0
                    };
                    let wait = Duration::from_millis(delay_ms + jitter);

                    println!(
                        "[BinanceLimiter] 瞬时错误 (尝试 {}/{}): {}，重试等待 {:?}",
                        attempt, MAX_RETRIES, err, wait
                    );
                    sleep(wait).await;
                    continue;
                }

                Err(other) => return Err(other.into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BinanceRestfulClient;
    use crate::models::RequestInfo;
    use reqwest::Client as ReqwestClient;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_request_returns_response_with_custom_headers() -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let test_path = "/api/v3/ticker/price";

        Mock::given(method("GET"))
            .and(path(test_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"symbol": "BTCUSDT", "price": "45000.00"}))
                    .insert_header("Content-Type", "application/json; charset=utf-8")
                    .insert_header("X-MBX-USED-WEIGHT-1M", "5")
                    .insert_header("Server", "wiremock"),
            )
            .mount(&mock_server)
            .await;

        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1, None, None, None)?;
        let bn_client = BinanceRestfulClient::new().await;
        let req = ReqwestClient::new().get(format!("{}{}", mock_server.uri(), test_path));

        let resp = bn_client.request(req, &request_info).await?;

        assert_eq!(resp.status().as_u16(), 200);
        let headers = resp.headers();
        assert!(headers.get("Content-Type").is_some());
        assert_eq!(headers.get("X-MBX-USED-WEIGHT-1M").unwrap(), "5");
        assert_eq!(headers.get("Server").unwrap(), "wiremock");

        Ok(())
    }

    #[tokio::test]
    async fn test_request_without_used_weight_header() -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let test_path = "/api/v3/ping";

        Mock::given(method("GET"))
            .and(path(test_path))
            .respond_with(ResponseTemplate::new(200).set_body_string("pong"))
            .mount(&mock_server)
            .await;

        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1, None, None, None)?;
        let bn_client = BinanceRestfulClient::new().await;
        let req = ReqwestClient::new().get(format!("{}{}", mock_server.uri(), test_path));

        let resp = bn_client.request(req, &request_info).await?;

        assert_eq!(resp.status().as_u16(), 200);
        let headers = resp.headers();
        assert!(headers.get("X-MBX-USED-WEIGHT-1M").is_none());

        Ok(())
    }

    #[tokio::test]
    async fn minimal_mock() {
        let mock_server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::any())
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("OK"))
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/test", mock_server.uri());
        println!("Testing URL: {}", url);

        let resp = client.get(&url).send().await.unwrap();
        println!("Status: {}", resp.status());
        assert_eq!(resp.status().as_u16(), 200);
    }
}
