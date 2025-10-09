use crate::http_client::DefaultRateLimiter;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::collections::HashMap;
use url::Url;

///
/// PLAN：去除交易所之类的类的定义，因为发现这样没有办法统一

//不太确定哪个好，就先用这个用于高精度计算
pub type Decimal = rust_decimal::Decimal;

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
    request_timeout_mill_secs: u32,
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
