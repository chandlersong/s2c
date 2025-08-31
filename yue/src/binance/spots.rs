use crate::binance::bn_models::{
    ExchangeInfo, EmptyQueryParams, ToQueryParams,
};
use crate::binance::bn_restful_commands::{execute_bn_get, EXCHANGE_INFO_COMMAND};
use crate::errors::YueError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingSymbolInfo {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KlineInterval {
    OneSecond,
    OneMinute,
    ThreeMinutes,
    FiveMinutes,
    FifteenMinutes,
    ThirtyMinutes,
    OneHour,
    TwoHours,
    FourHours,
    SixHours,
    EightHours,
    TwelveHours,
    OneDay,
    ThreeDays,
    OneWeek,
    OneMonth,
}

impl AsRef<str> for KlineInterval {
    fn as_ref(&self) -> &str {
        match self {
            KlineInterval::OneSecond => "1s",
            KlineInterval::OneMinute => "1m",
            KlineInterval::ThreeMinutes => "3m",
            KlineInterval::FiveMinutes => "5m",
            KlineInterval::FifteenMinutes => "15m",
            KlineInterval::ThirtyMinutes => "30m",
            KlineInterval::OneHour => "1h",
            KlineInterval::TwoHours => "2h",
            KlineInterval::FourHours => "4h",
            KlineInterval::SixHours => "6h",
            KlineInterval::EightHours => "8h",
            KlineInterval::TwelveHours => "12h",
            KlineInterval::OneDay => "1d",
            KlineInterval::ThreeDays => "3d",
            KlineInterval::OneWeek => "1w",
            KlineInterval::OneMonth => "1M",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KlineParams {
    pub symbol: String,
    pub interval: KlineInterval,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub limit: Option<u32>,
}

impl KlineParams {
    pub fn new(symbol: String) -> Self {
        Self {
            symbol,
            interval: KlineInterval::OneHour,
            start_time: None,
            end_time: None,
            limit: None,
        }
    }
}

impl ToQueryParams for KlineParams {
    fn to_query_string(&self) -> String {
        let mut params = vec![];
        params.push(format!("symbol={}", self.symbol));
        params.push(format!("interval={}", self.interval.as_ref()));
        if let Some(start) = self.start_time {
            params.push(format!("startTime={}", start));
        }
        if let Some(end) = self.end_time {
            params.push(format!("endTime={}", end));
        }
        if let Some(limit) = self.limit {
            params.push(format!("limit={}", limit));
        }
        params.join("&")
    }
}

/// 获取现货交易对信息
/// 
/// # 参数
/// * `status` - 交易对状态过滤器
///   - `None` 或 `Some("TRADING")`: 只返回交易中的交易对 (默认)
///   - `Some("ALL")`: 返回所有交易对 (不进行状态过滤)
///   - `Some("HALT")`: 只返回暂停交易的交易对
///   - 其他值: 按指定状态过滤
/// 
/// # 返回
/// 返回符合条件的交易对信息列表，包含 symbol, status, base_asset, quote_asset_precision, order_types
pub async fn get_trading_spot_symbols(status: Option<&str>) -> Result<Vec<TradingSymbolInfo>, YueError> {
    let exchange_info: ExchangeInfo = execute_bn_get::<EmptyQueryParams, ExchangeInfo>(
        &EXCHANGE_INFO_COMMAND,
        None,
        None,
    )
    .await?;

    let filter_status = status.unwrap_or("TRADING");

    let trading_symbols: Vec<TradingSymbolInfo> = if filter_status == "ALL" {
        // Return all symbols without filtering
        exchange_info
            .symbols
            .into_iter()
            .map(|symbol| TradingSymbolInfo {
                symbol: symbol.symbol,
                status: symbol.status,
                base_asset: symbol.base_asset,
                quote_asset: symbol.quote_asset,
                quote_asset_precision: symbol.quote_asset_precision,
                order_types: symbol.order_types,
            })
            .collect()
    } else {
        // Filter by specified status
        exchange_info
            .symbols
            .into_iter()
            .filter(|symbol| symbol.status == filter_status)
            .map(|symbol| TradingSymbolInfo {
                symbol: symbol.symbol,
                status: symbol.status,
                base_asset: symbol.base_asset,
                quote_asset: symbol.quote_asset,
                quote_asset_precision: symbol.quote_asset_precision,
                order_types: symbol.order_types,
            })
            .collect()
    };

    Ok(trading_symbols)
}
