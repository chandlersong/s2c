use governor::Jitter;
use governor::RateLimiter;
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use li::tools::time::unix_time_now_u64_utc;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::time::timeout;
use url::Url;

///
/// PLAN：去除交易所之类的类的定义，因为发现这样没有办法统一

//不太确定哪个好，就先用这个用于高精度计算
pub type Decimal = rust_decimal::Decimal;
#[macro_export]
macro_rules! dec_from_opt_f64 {
    ($e:expr) => {
        crate::models::Decimal::from_f64($e).unwrap_or_default()
    };
}

pub fn create_empty_param() -> Option<EmptyObject> {
    Option::from(EmptyObject {})
}
#[derive(Debug, PartialEq, Default)]
pub struct EmptyObject;

impl std::fmt::Display for EmptyObject {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "")
    }
}

impl Serialize for EmptyObject {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let empty_map: HashMap<String, serde_json::Value> = HashMap::new();
        empty_map.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EmptyObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let empty_map: HashMap<String, serde_json::Value> = HashMap::deserialize(deserializer)?;
        if empty_map.is_empty() {
            Ok(EmptyObject {})
        } else {
            Err(de::Error::custom("Expected an empty JSON object"))
        }
    }
}

pub type DefaultRateLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

///
/// 主要是参考币安的经验。这个HostInfo主要是和限流和地址相关信息。
///
/// 1. 主要提供方法。
///
/// 1. 获得host，直接把HostInfo转换成host，作为String输出
/// 2. 提供更新max_limit和获得max_limit以u32形式的方法
/// 3. 更新和获得limiter的方法。以及获得令牌的方法。
///
///
#[derive(Debug, Clone)]
pub struct HostInfo {
    host: String,
    max_limit: Arc<AtomicU32>,
    limiter: Arc<RwLock<Arc<DefaultRateLimiter>>>,
}

impl AsRef<str> for HostInfo {
    fn as_ref(&self) -> &str {
        &self.host
    }
}

impl<'a> From<&'a HostInfo> for &'a str {
    fn from(value: &'a HostInfo) -> Self {
        &value.host
    }
}

impl HostInfo {
    pub fn new<S: AsRef<str>>(host: S, max_limit: u32, limiter: DefaultRateLimiter) -> Self {
        Self {
            host: host.as_ref().to_string(),
            max_limit: Arc::new(AtomicU32::new(max_limit)),
            limiter: Arc::new(RwLock::new(Arc::new(limiter))),
        }
    }

    pub fn host_as_str(&self) -> &str {
        &self.host
    }

    pub fn set_max_limit(&self, v: u32) {
        self.max_limit.store(v, Ordering::SeqCst);
    }

    pub fn get_max_limit(&self) -> u32 {
        self.max_limit.load(Ordering::SeqCst)
    }

    pub fn set_limiter(&self, limiter: DefaultRateLimiter) {
        if let Ok(mut guard) = self.limiter.write() {
            *guard = Arc::new(limiter);
        }
    }

    pub async fn acquire_limit_token(&self, weight: u32, timeout_secs: u64) -> Result<(), crate::errors::YueError> {
        let timeout_duration = Duration::from_secs(timeout_secs);

        let weight_nz = match NonZeroU32::new(weight) {
            Some(w) => w,
            None => return Err(crate::errors::YueError::new("权重必须为非零")),
        };

        // clone an Arc handle to the limiter so we don't hold the RwLock across .await
        let limiter_cloned = {
            let guard = self.limiter.read().map_err(|_| crate::errors::YueError::new("锁定限流器失败"))?;
            guard.clone()
        };

        let jitter = Jitter::up_to(Duration::from_millis(500));

        match timeout(timeout_duration, limiter_cloned.until_n_ready_with_jitter(weight_nz, jitter)).await {
            Err(e) => Err(crate::errors::YueError::new(&format!("限流超时: {}", e))),
            Ok(res) => match res {
                Ok(_) => Ok(()),
                Err(e) => Err(crate::errors::YueError::new(&format!("限流器内部错误: {:?}", e))),
            },
        }
    }
}

/// NEXT：加入一个generate方法，参数为weight和timeout
/// 因为这两个可能是多变的。比如在获取swap kline的过程中，会根据limit进行变更
#[derive(Debug, Clone)]
pub struct RequestInfo {
    inner: Url,
    pub has_security: bool,
    pub weight: u32,
    pub rate_limit: Option<&'static DefaultRateLimiter>,
    pub request_timeout_mill_secs: u32,
    rate_limit_timeout_secs: u64,
}

impl RequestInfo {
    // 直接从完整 URL 构建
    pub fn new_full_url<S: AsRef<str>>(
        full_url: S,
        has_security: bool,
        weight: u32,
        rate_limit: Option<&'static DefaultRateLimiter>,
        request_timeout_mill_secs: Option<u32>,
        rate_limit_timeout_secs: Option<u64>,
    ) -> Result<Self, url::ParseError> {
        let inner = Url::parse(full_url.as_ref())?;
        Ok(Self {
            inner,
            has_security,
            weight,
            rate_limit,
            request_timeout_mill_secs: request_timeout_mill_secs.unwrap_or_else(|| 1000u32),
            rate_limit_timeout_secs: rate_limit_timeout_secs.unwrap_or_else(|| 2),
        })
    }

    // 从 base + path 构建（内部负责安全拼接）
    pub fn from_base_path<B: AsRef<str>, P: AsRef<str>>(
        base: B,
        path: P,
        has_security: bool,
        weight: u32,
        rate_limit: Option<&'static DefaultRateLimiter>,
        request_timeout_mill_secs: Option<u32>,
        rate_limit_timeout_secs: Option<u64>,
    ) -> Result<Self, url::ParseError> {
        let base = base.as_ref().trim_end_matches('/');
        let path = path.as_ref();
        let full = if path.starts_with('/') {
            format!("{base}{path}")
        } else {
            format!("{base}/{path}")
        };
        Self::new_full_url(full, has_security, weight, rate_limit, request_timeout_mill_secs, rate_limit_timeout_secs)
    }

    pub fn clone_with_weight(&mut self, new_weight: u32) -> Self {
        Self {
            inner: self.inner.clone(),
            has_security: self.has_security,
            weight: new_weight,
            rate_limit: self.rate_limit,
            request_timeout_mill_secs: self.request_timeout_mill_secs,
            rate_limit_timeout_secs: self.rate_limit_timeout_secs,
        }
    }

    // 如需获取内部 Url 的只读引用
    pub fn url(&self) -> &Url {
        &self.inner
    }

    // 字符串视图
    pub fn as_str(&self) -> &str {
        self.inner.as_str()
    }

    pub fn get_rate_limit_timeout(&self) -> u64 {
        self.rate_limit_timeout_secs
    }
}

impl AsRef<Url> for RequestInfo {
    fn as_ref(&self) -> &Url {
        &self.inner
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HistoryInterval {
    OneMinute,
    FiveMinutes,
    OneHour,
}

impl AsRef<str> for HistoryInterval {
    fn as_ref(&self) -> &str {
        match self {
            HistoryInterval::OneMinute => "1m",
            HistoryInterval::FiveMinutes => "5m",
            HistoryInterval::OneHour => "1h",
        }
    }
}

impl HistoryInterval {
    pub fn to_milliseconds(&self) -> u64 {
        match self {
            HistoryInterval::OneMinute => 60 * 1000,
            HistoryInterval::FiveMinutes => 5 * 60 * 1000,
            HistoryInterval::OneHour => 60 * 60 * 1000,
        }
    }

    ///
    /// 获得传入一个时间戳，最近的时间符合的时间unix mill second
    /// 比如传入 10:12:33
    /// 那么
    /// 1m: 返回 10:12:00的 unix ms
    /// 5m: 返回 10:10:00的 unix ms
    /// 1h: 返回 10:00:00的 unix ms
    ///
    pub fn get_close_unix_ms(&self, timestamp: u64) -> u64 {
        // 获取当前时间的 unix 毫秒，若出错则返回 0
        let interval_ms = self.to_milliseconds();
        // 向下取整到 interval 边界
        (timestamp / interval_ms) * interval_ms
    }

    pub fn get_now_close_unix_ms_utc(&self) -> u64 {
        self.get_close_unix_ms(unix_time_now_u64_utc())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{DefaultRateLimiter, HistoryInterval, HostInfo};
    use governor::Quota;
    use std::num::NonZeroU32;

    /// 测试：HistoryInterval::get_close_unix_ms 在一分钟间隔下的对齐
    ///
    /// 设计思路：验证get_close_unix_ms能够正确将任意时间戳对齐到interval边界
    ///
    /// 场景说明：
    /// - 传入当前时间戳
    /// - 验证返回的时间戳对齐到1分钟边界
    /// - 返回的时间戳不应超过传入的时间戳
    /// - 两者间的差距应小于1分钟
    #[test]
    fn test_get_close_unix_ms_one_minute() {
        let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let ts = HistoryInterval::OneMinute.get_close_unix_ms(now_ms);
        assert!(ts <= now_ms, "返回的时间不应在未来");
        assert_eq!(ts % (60 * 1000), 0, "应对齐到整分钟");
        assert!(now_ms - ts < 60 * 1000, "差距应小于 1 分钟");
    }

    /// 测试：HistoryInterval::get_close_unix_ms 在五分钟间隔下的对齐
    ///
    /// 设计思路：验证get_close_unix_ms能够正确将任意时间戳对齐到5分钟边界
    ///
    /// 场景说明：
    /// - 传入当前时间戳
    /// - 验证返回的时间戳对齐到5分钟边界
    /// - 返回的时间戳不应超过传入的时间戳
    /// - 两者间的差距应小于5分钟
    #[test]
    fn test_get_close_unix_ms_five_minutes() {
        let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let ts = HistoryInterval::FiveMinutes.get_close_unix_ms(now_ms);
        assert!(ts <= now_ms, "返回的时间不应在未来");
        assert_eq!(ts % (5 * 60 * 1000), 0, "应对齐到 5 分钟边界");
        assert!(now_ms - ts < 5 * 60 * 1000, "差距应小于 5 分钟");
    }

    /// 测试：HistoryInterval::get_close_unix_ms 在一小时间隔下的对齐
    ///
    /// 设计思路：验证get_close_unix_ms能够正确将任意时间戳对齐到1小时边界
    ///
    /// 场景说明：
    /// - 传入当前时间戳
    /// - 验证返回的时间戳对齐到1小时边界
    /// - 返回的时间戳不应超过传入的时间戳
    /// - 两者间的差距应小于1小时
    #[test]
    fn test_get_close_unix_ms_one_hour() {
        let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let ts = HistoryInterval::OneHour.get_close_unix_ms(now_ms);
        assert!(ts <= now_ms, "返回的时间不应在未来");
        assert_eq!(ts % (60 * 60 * 1000), 0, "应对齐到整小时");
        assert!(now_ms - ts < 60 * 60 * 1000, "差距应小于 1 小时");
    }

    #[tokio::test]
    async fn test_hostinfo_basic_and_acquire() {
        let quota = Quota::per_minute(NonZeroU32::new(100).unwrap());
        let limiter = DefaultRateLimiter::direct(quota);

        let host = HostInfo::new("https://api.test", 1000, limiter);

        assert_eq!(host.host_as_str(), "https://api.test");
        assert_eq!(host.get_max_limit(), 1000);

        host.set_max_limit(500);
        assert_eq!(host.get_max_limit(), 500);

        // acquire should succeed for weight 1
        let res = host.acquire_limit_token(1, 2).await;
        assert!(res.is_ok(), "首次获取令牌应成功");
    }

    #[tokio::test]
    async fn test_acquire_timeout_behavior() {
        let quota = Quota::per_minute(NonZeroU32::new(1).unwrap());
        let limiter = DefaultRateLimiter::direct(quota);

        let host = HostInfo::new("https://api.test", 10, limiter);

        // first acquire should succeed
        let r1 = host.acquire_limit_token(1, 1).await;
        assert!(r1.is_ok(), "第一次获取令牌应成功");

        // second acquire with short timeout should fail due to token refill being long
        let r2 = host.acquire_limit_token(1, 1).await;
        assert!(r2.is_err(), "在短超时时间内第二次获取应失败");

        // zero weight should return error
        let r3 = host.acquire_limit_token(0, 1).await;
        assert!(r3.is_err(), "权重为0应当报错");
    }
}
