use crate::binance::bn_models::spot_websocket::ExecutionReportPayload;
use crate::models::{Decimal, RequestInfo};
use actix::Message;
use li::tools::time::{UnixTimeStamp, unix_time_now_u64_utc};
use reqwest::RequestBuilder;
use serde::de::{DeserializeOwned, Error};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use std::fmt::Display;
use std::str::FromStr;

// 查询参数trait定义
pub trait ToRequestBuilder {
    fn to_request_builder(&self, request_info: &RequestInfo) -> RequestBuilder;
}
#[derive(Clone, Copy, Debug)]
pub enum SymbolType {
    Spot,   //现货
    Swap,   //永续
    Future, //交割合约
    Option, //期权
}

impl From<SymbolType> for &'static str {
    fn from(s: SymbolType) -> &'static str {
        match s {
            SymbolType::Spot => "Spot",
            SymbolType::Swap => "Swap",
            SymbolType::Future => "Future",
            SymbolType::Option => "Option",
        }
    }
}

impl Display for SymbolType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: &'static str = (*self).into();
        write!(f, "{}", s)
    }
}

pub trait HistoryVo: DeserializeOwned + Sync + Send + Clone {
    fn get_close_time(&self) -> u64;

    fn get_open_time(&self) -> u64;

    //有些需要更新，有些则不需要。
    fn update_symbol(&mut self, _: &str) {}
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

/// 币安用户数据流 Listen Key 响应
/// POST /api/v3/userDataStream 或 POST /fapi/v1/listenKey 的响应
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ListenKeyResponse {
    #[serde(rename = "listenKey")]
    pub listen_key: String,
}

pub type SpotOrderData = AccountData<ExecutionReportPayload>;
pub type SwapOrderData = AccountData<crate::binance::bn_models::swap_account_stream::OrderTradeUpdatePayload>;
pub type PortfolioSpotOrderData = AccountData<crate::binance::bn_models::portfolio_account_websocket::ExecutionReportPayload>; // 先用同一个结构体占位，后续如果需要可以改成不同的结构体
pub type PortfolioSwapOrderData = AccountData<crate::binance::bn_models::portfolio_account_websocket::OrderTradeUpdatePayload>; // 先用同一个结构体占位，后续如果需要可以改成不同的结构体
#[derive(Debug, Serialize, Clone, Message)]
#[rtype(result = "()")]
pub struct AccountData<T: Clone + Message + DeserializeOwned> {
    pub account_name: String,
    pub data: T,
}

impl<T: Clone + Message + DeserializeOwned> AccountData<T> {
    pub fn new(account_name: &str, data: T) -> Self {
        Self {
            account_name: account_name.to_string(),
            data,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    /// 交易对符号，如 "BTCUSDT"
    pub symbol: String,
    /// 交易状态，可能的值包括：TRADING, END_OF_DAY, HALT, BREAK
    pub status: String,
    /// 基础资产，如 "BTC"
    pub base_asset: String,
    /// 报价资产，如 "USDT"
    pub quote_asset: String,
    /// 报价资产精度
    pub quote_asset_precision: i32,
    /// 支持的订单类型数组
    pub order_types: Vec<String>,
    /// 类型字段，spot统一填"spot"，swap填contract_type
    pub symbol_type: String,
    /// 上线时间，单位毫秒时间戳，spot取不到，所以为None，swap有值
    pub on_board_time: Option<u64>,
}
