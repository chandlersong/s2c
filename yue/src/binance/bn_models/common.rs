use crate::models::Decimal;
use li::tools::time::{UnixTimeStamp, unix_time_now_u64_utc};
use serde::de::{DeserializeOwned, Error};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use std::str::FromStr;

// 查询参数trait定义
pub trait ToQueryParams {
    fn to_query_string(&self) -> String;
}

// BTreeMap实现ToQueryParams
impl ToQueryParams for std::collections::BTreeMap<&str, String> {
    fn to_query_string(&self) -> String {
        self.iter().map(|(k, v)| format!("{}={}", k, v)).collect::<Vec<String>>().join("&")
    }
}

pub enum SymbolType {
    Spot,
    Swap,
}

pub struct EmptyQueryParams;

impl ToQueryParams for EmptyQueryParams {
    fn to_query_string(&self) -> String {
        String::new()
    }
}

pub trait HistoryVo: DeserializeOwned {
    fn get_close_time(&self) -> u64;

    fn get_open_time(&self) -> u64;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerTime {
    #[serde(rename = "serverTime")]
    pub time: UnixTimeStamp,
}

pub struct TimeStampRequest {
    pub timestamp: UnixTimeStamp,
    pub rec_window: u16,
}

impl fmt::Display for TimeStampRequest {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "timestamp={}&recvWindow={}", self.timestamp, self.rec_window)
    }
}

impl Default for TimeStampRequest {
    fn default() -> Self {
        TimeStampRequest {
            timestamp: unix_time_now_u64_utc(),
            rec_window: 5000,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WsCommandResponse {
    #[serde(rename = "id")]
    pub id: String,

    #[serde(rename = "result")]
    pub result: Option<String>,
}

pub trait SymbolInfoTrait {
    fn symbol(&self) -> &str;
    fn status(&self) -> &str;
    fn base_asset(&self) -> &str;
    fn quote_asset(&self) -> &str;
    fn order_types(&self) -> &Vec<String>;
    fn quote_precision(&self) -> i32;
    fn symbol_type(&self) -> &str;

    fn get_on_board_time(&self) -> Option<u64> {
        None
    }
}

pub trait ExchangeInfoTrait {
    type SymbolInfo: SymbolInfoTrait;
    fn timezone(&self) -> &str;
    fn server_time(&self) -> u64;
    fn symbols(&self) -> &Vec<Self::SymbolInfo>;
}

pub fn map_depth_levels<'de, D>(deserializer: D) -> Result<Vec<(f64, f64)>, D::Error>
where
    D: Deserializer<'de>,
{
    // 先按字符串解析
    let raw: Vec<(String, String)> = Vec::<(String, String)>::deserialize(deserializer)?;
    let mut out = Vec::with_capacity(raw.len());
    for (p, q) in raw.into_iter() {
        let price = p.parse::<f64>().map_err(|e| D::Error::custom(format!("price parse error: {}", e)))?;
        let qty = q.parse::<f64>().map_err(|e| D::Error::custom(format!("qty parse error: {}", e)))?;
        out.push((price, qty));
    }
    Ok(out)
}

pub fn map_depth_levels_decimal<'de, D>(deserializer: D) -> Result<Vec<(Decimal, Decimal)>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Vec<(String, String)> = Vec::<(String, String)>::deserialize(deserializer)?;
    let mut out = Vec::with_capacity(raw.len());
    for (p, q) in raw.into_iter() {
        let price = Decimal::from_str(&p).map_err(|e| D::Error::custom(format!("price parse error: {}", e)))?;
        let qty = Decimal::from_str(&q).map_err(|e| D::Error::custom(format!("qty parse error: {}", e)))?;
        out.push((price, qty));
    }
    Ok(out)
}
