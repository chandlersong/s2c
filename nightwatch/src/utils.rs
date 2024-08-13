use crate::models::UnixTimeStamp;
use hmac::digest::InvalidLength;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Deserializer};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

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

pub fn setup_logger() -> Result<(), fern::InitError> {
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
        .level(log::LevelFilter::Debug)
        .chain(std::io::stdout())
        .apply()?;
    Ok(())
}