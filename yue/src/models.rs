use crate::http_client::DefaultRateLimiter;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::collections::HashMap;
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
}

#[cfg(test)]
mod tests {
    use crate::models::HistoryInterval;

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
}
