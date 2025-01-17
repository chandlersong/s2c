use crate::models::UnixTimeStamp;
use hmac::digest::InvalidLength;
use hmac::{Hmac, Mac};
use log::LevelFilter;
#[cfg(test)]
use serde::de;
use serde::{Deserialize, Deserializer};
use sha2::Sha256;
use sonyflake::Sonyflake;
#[cfg(test)]
use std::fs;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn unix_time() -> UnixTimeStamp {
    let now = SystemTime::now();
    let since_epoch = now.duration_since(UNIX_EPOCH).unwrap();
    since_epoch.as_secs() * 1000 + u64::from(since_epoch.subsec_nanos()) / 1_000_000
}

// 自定义反序列化函数，将字符串属性转换为数字
pub(crate) fn str_to_u16<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    s.parse::<u16>().map_err(serde::de::Error::custom)
}


// 签名方法从官方项目copy https://github.com/binance/binance-spot-connector-rust/blob/main/src/utils.rs#L9
pub(crate) fn sign_hmac(payload: &str, key: &str) -> Result<String, InvalidLength> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())?;

    mac.update(payload.to_string().as_bytes());
    let result = mac.finalize();
    Ok(format!("{:x}", result.into_bytes()))
}


pub fn setup_logger(level: Option<LevelFilter>) -> Result<(), fern::InitError> {
    let filter = match level {
        None => { LevelFilter::Debug }
        Some(v) => { v }
    };
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}] {}",
                humantime::format_rfc3339_seconds(SystemTime::now()),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(filter)
        .chain(std::io::stdout())
        .apply()?;
    Ok(())
}


pub struct SnowyFlakeWrapper {
    sf: Mutex<Sonyflake>,
}

impl SnowyFlakeWrapper {
    pub fn new() -> SnowyFlakeWrapper {
        let sf = Sonyflake::new().unwrap();
        SnowyFlakeWrapper {
            sf: Mutex::new(sf),
        }
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

pub mod string_to_int {
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

