use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Deserializer};
use crate::models::UnixTimeStamp;

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
