use crate::models::UnixTimeStamp;
use chrono::{DateTime, Utc};
use hmac::digest::InvalidLength;
use hmac::{Hmac, Mac};
use log::error;
#[cfg(test)]
use serde::de;
use serde::{Deserialize, Deserializer};
use sha2::Sha256;
use sonyflake::Sonyflake;
#[cfg(test)]
use std::fs;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, watch};
use tokio::time;

pub fn unix_2_readable(unix_timestamp_millis: &u64) -> DateTime<Utc> {
    // Unix 时间戳（毫秒）

    // 将毫秒转换为秒和纳秒
    let seconds = (unix_timestamp_millis / 1000) as u64;
    let nanoseconds = ((unix_timestamp_millis % 1000) * 1_000_000) as u32;

    // 创建 SystemTime
    let system_time = UNIX_EPOCH + std::time::Duration::new(seconds, nanoseconds);

    system_time.into()
}
pub fn unix_time() -> UnixTimeStamp {
    let now = SystemTime::now();
    let since_epoch = now.duration_since(UNIX_EPOCH).unwrap();
    since_epoch.as_secs() * 1000 + u64::from(since_epoch.subsec_nanos()) / 1_000_000
}

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

pub struct SnowyFlakeWrapper {
    sf: Mutex<Sonyflake>,
}

impl SnowyFlakeWrapper {
    pub fn new() -> SnowyFlakeWrapper {
        let sf = Sonyflake::new().unwrap();
        SnowyFlakeWrapper { sf: Mutex::new(sf) }
    }

    pub fn next_id_string(&self) -> String {
        let mut value = self.sf.lock().unwrap();
        value.next_id().unwrap().to_string()
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
mod tests {
    use crate::tools::{FrequencyReducer, unix_2_readable};
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
    fn test_unix_2_time() {
        let expected = format!("{}", unix_2_readable(&1737093025292));
        assert_eq!("2025-01-17 05:50:25.292 UTC", expected);
    }
}
