use crate::errors::YueError;
use crate::models::create_share_rate_limiter;
#[cfg(test)]
use crate::models::{DefaultRateLimiter, HostInfo};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STD};
use ed25519_dalek::SigningKey;
use ed25519_dalek::ed25519::signature::SignerMut;
use ed25519_dalek::pkcs8::DecodePrivateKey; // 带 pem 支持
#[cfg(test)]
use governor::Quota;
use hmac::digest::InvalidLength;
use hmac::{Hmac, Mac};
use log::error;
#[cfg(test)]
use serde::de;
use serde::{Deserialize, Deserializer};
use sha2::Sha256;
use sonyflake::Sonyflake;
use std::fs;
#[cfg(test)]
use std::num::NonZeroU32;
#[cfg(test)]
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
#[cfg(test)]
use tokio::sync::RwLock;
use tokio::sync::{broadcast, watch};
use tokio::time;

// 自定义反序列化函数，将字符串属性转换为数字
pub fn str_to_u16<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    s.parse::<u16>().map_err(serde::de::Error::custom)
}

// 签名方法从官方项目copy https://github.com/binance/binance-spot-connector-rust/blob/main/src/utils.rs#L9
pub fn sign_hmac(payload: &str, key: &str) -> Result<String, InvalidLength> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())?;

    mac.update(payload.to_string().as_bytes());
    let result = mac.finalize();
    Ok(format!("{:x}", result.into_bytes()))
}

pub trait SignatureContext {
    fn context_for_signature(&self) -> String;
}

impl SignatureContext for &str {
    fn context_for_signature(&self) -> String {
        self.to_string()
    }
}

impl SignatureContext for String {
    fn context_for_signature(&self) -> String {
        self.clone()
    }
}

/// 使用 Ed25519 对 payload 做签名，返回 BASE64 编码字符串。
pub fn sign_ed25519<T: SignatureContext>(payload: T, signing_key: &mut SigningKey) -> Result<String, YueError> {
    let payload_str = payload.context_for_signature();
    let signature = signing_key.sign(payload_str.as_bytes());
    Ok(BASE64_STD.encode(signature.to_bytes()))
}

pub fn load_ed25519_signing_key(path: &str) -> Result<SigningKey, YueError> {
    // 方式1：直接从 PEM 文件读取（最推荐）
    let pem_content = fs::read_to_string(path)?;

    let signing_key = SigningKey::from_pkcs8_pem(&pem_content)?;

    Ok(signing_key)
}
static SNOW_FLAKE: OnceLock<SnowyFlakeWrapper> = OnceLock::new();

pub fn get_snow_flake_id_string() -> String {
    SNOW_FLAKE.get_or_init(|| SnowyFlakeWrapper::new()).next_id_string()
}

pub fn get_snow_flake_id_u64() -> u64 {
    SNOW_FLAKE.get_or_init(|| SnowyFlakeWrapper::new()).next_id_u64()
}

pub struct SnowyFlakeWrapper {
    sf: Mutex<Sonyflake>,
}

impl SnowyFlakeWrapper {
    pub fn new() -> SnowyFlakeWrapper {
        let sf = Sonyflake::new().unwrap();
        SnowyFlakeWrapper { sf: Mutex::new(sf) }
    }

    pub fn next_id_string(&self) -> String {
        let value = self.sf.lock().unwrap();
        value.next_id().unwrap().to_string()
    }

    pub fn next_id_u64(&self) -> u64 {
        let value = self.sf.lock().unwrap();
        value.next_id().unwrap()
    }
}

pub mod string_to_float {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::str::FromStr;

    pub fn serialize<S>(value: &f64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_f64(*value)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<f64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        f64::from_str(&s).map_err(serde::de::Error::custom)
    }
}

pub mod string_to_u64 {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::str::FromStr;

    pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(*value)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        u64::from_str(&s).map_err(serde::de::Error::custom)
    }
}

pub mod string_to_option_float {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::str::FromStr;

    pub fn serialize<S>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(v) => serializer.serialize_f64(*v),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<String>::deserialize(deserializer)?;
        match opt {
            Some(s) if s.trim().is_empty() => Ok(None),
            Some(s) => f64::from_str(&s).map(Some).map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

pub mod string_to_decimal {
    use rust_decimal::Decimal;
    use serde::{Deserialize, Deserializer, Serializer};
    use std::str::FromStr;

    pub fn serialize<S>(value: &Decimal, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Decimal::from_str(&s).map_err(serde::de::Error::custom)
    }
}

pub mod string_to_option_decimal {
    use rust_decimal::Decimal;
    use serde::{Deserialize, Deserializer, Serializer};
    use std::str::FromStr;

    pub fn serialize<S>(value: &Option<Decimal>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(v) => serializer.serialize_str(&v.to_string()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Decimal>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<String>::deserialize(deserializer)?;
        match opt {
            Some(s) if s.trim().is_empty() => Ok(None),
            Some(s) => Decimal::from_str(&s).map(Some).map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
pub fn parse_test_json<T: for<'a> de::Deserialize<'a>>(path: &str) -> T {
    let json = fs::read_to_string(path).unwrap();
    serde_json::from_str(&json).unwrap()
}

/// `FrequencyReducer` 用来把速度来进行降频处理。
/// 比如在处理websocket信息的时候，因为所有信息都是1s一次，如果后续做不出来的时候，就有可能出现堵塞。
/// 比如保存的时候，其实主要保存分钟信息就好了。没有必要保存所有信息。
/// 所以做了这个奖品的操作。
/// 这里，是以最新的数据为准
///
/// 这样做的主要原因是：
/// 1. 因为是协程，应该不是很大。
/// 2. Websocket的数据是每秒推送。同时，只会推送过去1s的数据。如果每个都存，没有丢弃机制，可能会堵塞。
/// 3. 如果每一次推送，作为一个整体。因为那么些
#[derive(Clone)]
pub struct FrequencyReducer<S: Send + Clone + Sync> {
    cache_tx: watch::Sender<Option<S>>,
}

impl<V: Send + Clone + Sync + 'static> FrequencyReducer<V> {
    pub async fn new(out_tx: broadcast::Sender<V>, frequency_mill_seconds: u64) -> Self {
        let (cache_tx, cache_rx) = watch::channel(None);
        let res = Self { cache_tx };
        tokio::spawn(async move {
            frequency_reducer_output(cache_rx, out_tx, frequency_mill_seconds).await;
        });
        res
    }

    pub async fn update(&mut self, value: V) {
        self.cache_tx.send(Some(value)).unwrap();
    }
}

async fn frequency_reducer_output<V: Send + Clone + Sync>(
    mut cache_rx: watch::Receiver<Option<V>>,
    out_tx: broadcast::Sender<V>,
    frequency_mill_seconds: u64,
) {
    let mut interval = time::interval(Duration::from_millis(frequency_mill_seconds));
    loop {
        interval.tick().await; // 等待下一个间隔
        if let Some(v) = cache_rx.borrow_and_update().clone() {
            match out_tx.send(v) {
                Ok(_) => {}
                Err(e) => {
                    error!("failed to send: {}", e);
                }
            };
        }
    }
}
#[cfg(test)]
pub fn create_mock_host_info(host: &str) -> Arc<HostInfo> {
    // 初始 quota（用一个合理默认值，马上会被刷新覆盖）
    let limiter = create_share_rate_limiter(300);
    Arc::new(HostInfo::new(host, 0, limiter))
}

#[cfg(test)]
mod tests {
    use crate::tools::{FrequencyReducer, sign_ed25519};
    use ed25519_dalek::SigningKey;
    use std::time::Duration;
    use tokio::sync::broadcast;
    use tokio::time;

    #[tokio::test]
    pub async fn test_update_cache() {
        let (tx, mut rx) = broadcast::channel(10);
        let reducer: FrequencyReducer<i32> = FrequencyReducer::new(tx.clone(), 1000).await;

        let mut reducer_clone = reducer.clone();
        tokio::spawn(async move {
            reducer_clone.update(1).await;
        });

        let timeout_duration = Duration::from_secs(3);
        match time::timeout(timeout_duration, rx.recv()).await {
            Ok(res) => match res {
                Ok(value) => {
                    assert_eq!(value, 1, "wrong value");
                }
                Err(_) => {
                    assert!(false, "not fresh");
                }
            },
            Err(_) => {
                assert!(false, "channel timeout");
            }
        }
    }

    /// 有一个新的出来后，旧的应该被替换掉。
    #[tokio::test]
    pub async fn test_update_cache_replace() {
        let (tx, mut rx) = broadcast::channel(10);
        let reducer: FrequencyReducer<i32> = FrequencyReducer::new(tx.clone(), 1000).await;

        let mut reducer_clone = reducer.clone();
        tokio::spawn(async move {
            reducer_clone.update(1).await;
            reducer_clone.update(2).await;
        });

        let timeout_duration = Duration::from_secs(3);
        match time::timeout(timeout_duration, rx.recv()).await {
            Ok(res) => match res {
                Ok(value) => {
                    assert_eq!(value, 2, "wrong value");
                }
                Err(_) => {
                    assert!(false, "not fresh");
                }
            },
            Err(_) => {
                assert!(false, "channel timeout");
            }
        }
    }

    #[test]
    fn test_sign_ed25519_fixture() {
        let mut signing_key = SigningKey::from_bytes(&[0u8; 32]);
        let payload = "apiKey=TEST&timestamp=123";
        let signature = sign_ed25519(payload, &mut signing_key).unwrap();
        assert_eq!(
            signature, "umh1gRymXHNSoDMHnRoNA2bDjc+lKf76K7azVXj6hNRzahxw0XDjUWMGuNlY+gi6NzL1YdJGY1susAd057iVCw==",
            "unexpected signature",
        );
    }

    #[test]
    fn test_sign_ed25519_empty_payload() {
        let mut signing_key = SigningKey::from_bytes(&[0u8; 32]);
        let signature = sign_ed25519("", &mut signing_key).unwrap();
        assert_eq!(
            signature, "j4lbPK/iyVBgOdDipmOCVoAEZ0/o0jd4UJLkDWqvSD5PxgFocF8x8QFZYTjOIao1fA0yoGT0I9w+5Ko6v1P4Aw==",
            "unexpected signature for empty payload",
        );
    }

    #[test]
    fn debug_signature_generation() {
        let base_query_string = "symbol=BTCUSDT";
        let api_secret = "test_api_secret";

        let generated_signature = super::sign_hmac(base_query_string, api_secret).unwrap();
        println!("Generated signature: {}", generated_signature);

        assert_eq!(generated_signature, "e383f8d24830bb711f0e833507b66798c5936a8fedd29b51bc5403cffd0ba755");
    }
}
