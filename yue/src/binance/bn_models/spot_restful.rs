use crate::binance::bn_models::common::map_depth_levels_decimal;
use crate::binance::bn_models::common::{ExchangeInfoTrait, HistoryVo, SymbolInfoTrait};
use crate::models::Decimal;
use crate::tools::string_to_decimal;
use serde::{Deserialize, Serialize};
/// 交易所信息结构体
/// 包含交易所的时区、服务器时间、速率限制规则、交易所过滤器和所有交易对的详细信息
#[derive(Deserialize, Debug)]
pub struct ExchangeInfo {
    #[serde(rename = "timezone")]
    /// 时区，通常为 "UTC"
    pub timezone: String,

    #[serde(rename = "serverTime")]
    /// 服务器当前时间戳（毫秒）
    pub server_time: u64,

    #[serde(rename = "rateLimits")]
    /// 速率限制规则数组
    pub rate_limits: Vec<RateLimit>,

    #[serde(rename = "exchangeFilters")]
    /// 交易所级过滤器数组
    pub exchange_filters: Vec<ExchangeFilter>,

    #[serde(rename = "symbols")]
    /// 交易对列表数组
    pub symbols: Vec<ExchangeSymbol>,
}

impl ExchangeInfoTrait for ExchangeInfo {
    type SymbolInfo = ExchangeSymbol;
    fn timezone(&self) -> &str {
        &self.timezone
    }
    fn server_time(&self) -> u64 {
        self.server_time
    }
    fn symbols(&self) -> &Vec<Self::SymbolInfo> {
        &self.symbols
    }
}

/// 速率限制规则结构体
/// 定义 API 调用的速率限制，包括类型、间隔和限制值
#[derive(Deserialize, Debug)]
pub struct RateLimit {
    #[serde(rename = "rateLimitType")]
    /// 限制类型，如 "REQUEST_WEIGHT"
    pub rate_limit_type: String,

    #[serde(rename = "interval")]
    /// 时间间隔，如 "MINUTE"
    pub interval: String,

    #[serde(rename = "intervalNum")]
    /// 间隔数量
    pub interval_num: i32,

    #[serde(rename = "limit")]
    /// 限制值
    pub limit: i32,
}

/// 交易所过滤器结构体
/// 定义交易所级别的过滤规则，使用标签区分不同类型
#[derive(Deserialize, Debug)]
#[serde(tag = "filterType")]
pub enum ExchangeFilter {
    #[serde(rename = "PRICE_FILTER")]
    /// 价格过滤器，包含最小/最大价格和价格步长
    PriceFilter {
        #[serde(rename = "minPrice", default)]
        min_price: Option<String>,
        #[serde(rename = "maxPrice", default)]
        max_price: Option<String>,
        #[serde(rename = "tickSize", default)]
        tick_size: Option<String>,
    },
    #[serde(rename = "LOT_SIZE")]
    /// 数量过滤器，包含最小/最大数量和数量步长
    LotSize {
        #[serde(rename = "minQty", default)]
        min_qty: Option<String>,
        #[serde(rename = "maxQty", default)]
        max_qty: Option<String>,
        #[serde(rename = "stepSize", default)]
        step_size: Option<String>,
    },
    #[serde(other)]
    /// 未知过滤器类型
    Unknown,
}
/// 交易对信息结构体
/// 包含单个交易对的详细信息，包括基本信息、状态、交易规则和权限
#[derive(Deserialize, Debug)]
pub struct ExchangeSymbol {
    #[serde(rename = "symbol")]
    /// 交易对符号，如 "BTCUSDT"
    pub symbol: String,
    #[serde(rename = "status")]
    /// 交易状态，可能的值包括：TRADING, END_OF_DAY, HALT, BREAK
    pub status: String,
    #[serde(rename = "baseAsset")]
    /// 基础资产，如 "BTC"
    pub base_asset: String,
    #[serde(rename = "baseAssetPrecision")]
    /// 基础资产精度
    pub base_asset_precision: i32,
    #[serde(rename = "quoteAsset")]
    /// 报价资产，如 "USDT"
    pub quote_asset: String,
    #[serde(rename = "quotePrecision")]
    /// 报价精度（即将废弃）
    pub quote_precision: i32,
    #[serde(rename = "quoteAssetPrecision")]
    /// 报价资产精度
    pub quote_asset_precision: i32,
    #[serde(rename = "baseCommissionPrecision")]
    /// 基础手续费精度
    pub base_commission_precision: i32,
    #[serde(rename = "quoteCommissionPrecision")]
    /// 报价手续费精度
    pub quote_commission_precision: i32,
    #[serde(rename = "orderTypes")]
    /// 支持的订单类型数组，可能的值包括：LIMIT, LIMIT_MAKER, MARKET, STOP_LOSS, STOP_LOSS_LIMIT, TAKE_PROFIT, TAKE_PROFIT_LIMIT
    pub order_types: Vec<String>,
    #[serde(rename = "icebergAllowed")]
    /// 是否允许冰山订单
    pub iceberg_allowed: bool,
    #[serde(rename = "ocoAllowed")]
    /// 是否允许 OCO 订单
    pub oco_allowed: bool,
    #[serde(rename = "quoteOrderQtyMarketAllowed")]
    /// 是否允许按报价数量下市价单
    pub quote_order_qty_market_allowed: bool,
    #[serde(rename = "allowTrailingStop")]
    /// 是否允许追踪止损
    pub allow_trailing_stop: bool,
    #[serde(rename = "cancelReplaceAllowed")]
    /// 是否允许取消并替换
    pub cancel_replace_allowed: bool,
    #[serde(rename = "isSpotTradingAllowed")]
    /// 是否允许现货交易
    pub is_spot_trading_allowed: bool,
    #[serde(rename = "isMarginTradingAllowed")]
    /// 是否允许保证金交易
    pub is_margin_trading_allowed: bool,
    /// 交易对级过滤器数组
    pub filters: Vec<ExchangeFilter>,
    /// 交易对权限数组
    pub permissions: Vec<String>,
    #[serde(rename = "defaultSelfTradePreventionMode")]
    /// 默认自成交预防模式
    pub default_self_trade_prevention_mode: String,
    #[serde(rename = "allowedSelfTradePreventionModes")]
    /// 允许的自成交预防模式数组
    pub allowed_self_trade_prevention_modes: Vec<String>,
}
impl SymbolInfoTrait for ExchangeSymbol {
    fn symbol(&self) -> &str {
        &self.symbol
    }
    fn status(&self) -> &str {
        &self.status
    }
    fn base_asset(&self) -> &str {
        &self.base_asset
    }
    fn quote_asset(&self) -> &str {
        &self.quote_asset
    }
    fn order_types(&self) -> &Vec<String> {
        &self.order_types
    }
    fn quote_precision(&self) -> i32 {
        self.quote_asset_precision
    }
    fn symbol_type(&self) -> &str {
        "spot"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinanceKline {
    #[serde(rename = "open_time")]
    pub open_time: u64, // 开盘时间戳 (毫秒)

    #[serde(rename = "open")]
    #[serde(with = "string_to_decimal")]
    pub open: Decimal, // 开盘价

    #[serde(rename = "high")]
    #[serde(with = "string_to_decimal")]
    pub high: Decimal, // 最高价

    #[serde(rename = "low")]
    #[serde(with = "string_to_decimal")]
    pub low: Decimal, // 最低价

    #[serde(rename = "close")]
    #[serde(with = "string_to_decimal")]
    pub close: Decimal, // 收盘价

    #[serde(rename = "volume")]
    #[serde(with = "string_to_decimal")]
    pub volume: Decimal, // 成交量

    #[serde(rename = "close_time")]
    pub close_time: u64, // 收盘时间戳 (毫秒)

    #[serde(rename = "quote_asset_volume")]
    #[serde(with = "string_to_decimal")]
    pub quote_asset_volume: Decimal, // 成交额

    #[serde(rename = "number_of_trades")]
    pub number_of_trades: u64, // 成交笔数

    #[serde(rename = "taker_buy_base_asset_volume")]
    #[serde(with = "string_to_decimal")]
    pub taker_buy_base_asset_volume: Decimal, // 主动买入成交量

    #[serde(rename = "taker_buy_quote_asset_volume")]
    #[serde(with = "string_to_decimal")]
    pub taker_buy_quote_asset_volume: Decimal, // 主动买入成交额

    #[serde(rename = "ignore")]
    pub ignore: String, // 忽略字段
}

impl HistoryVo for BinanceKline {
    fn get_close_time(&self) -> u64 {
        self.close_time
    }

    fn get_open_time(&self) -> u64 {
        self.open_time
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
/// 深度快照响应
pub struct Depth {
    #[serde(rename = "lastUpdateId")]
    pub last_update_id: u64,
    #[serde(rename = "bids", deserialize_with = "map_depth_levels_decimal")]
    pub bids: Vec<(Decimal, Decimal)>,
    #[serde(rename = "asks", deserialize_with = "map_depth_levels_decimal")]
    pub asks: Vec<(Decimal, Decimal)>,
}

/// 币安 /api/v3/ticker/24hr 24小时行情响应对象
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Ticker24hr {
    #[serde(rename = "symbol")]
    pub symbol: String, // 交易对，如 BTCUSDT

    #[serde(rename = "priceChange", with = "string_to_decimal")]
    pub price_change: Decimal, // 24小时价格变动

    #[serde(rename = "priceChangePercent", with = "string_to_decimal")]
    pub price_change_percent: Decimal, // 24小时价格变动百分比

    #[serde(rename = "weightedAvgPrice", with = "string_to_decimal")]
    pub weighted_avg_price: Decimal, // 24小时加权平均价

    #[serde(rename = "prevClosePrice", with = "string_to_decimal")]
    pub prev_close_price: Decimal, // 前一日收盘价

    #[serde(rename = "lastPrice", with = "string_to_decimal")]
    pub last_price: Decimal, // 最新成交价

    #[serde(rename = "lastQty", with = "string_to_decimal")]
    pub last_qty: Decimal, // 最新成交量

    #[serde(rename = "bidPrice", with = "string_to_decimal")]
    pub bid_price: Decimal, // 当前买一价

    #[serde(rename = "bidQty", with = "string_to_decimal")]
    pub bid_qty: Decimal, // 当前买一量

    #[serde(rename = "askPrice", with = "string_to_decimal")]
    pub ask_price: Decimal, // 当前卖一价

    #[serde(rename = "askQty", with = "string_to_decimal")]
    pub ask_qty: Decimal, // 当前卖一量

    #[serde(rename = "openPrice", with = "string_to_decimal")]
    pub open_price: Decimal, // 今日开盘价

    #[serde(rename = "highPrice", with = "string_to_decimal")]
    pub high_price: Decimal, // 24小时最高价

    #[serde(rename = "lowPrice", with = "string_to_decimal")]
    pub low_price: Decimal, // 24小时最低价

    #[serde(rename = "volume", with = "string_to_decimal")]
    pub volume: Decimal, // 24小时成交量

    #[serde(rename = "quoteVolume", with = "string_to_decimal")]
    pub quote_volume: Decimal, // 24小时成交额

    #[serde(rename = "openTime")]
    pub open_time: u64, // 统计开始时间（毫秒）

    #[serde(rename = "closeTime")]
    pub close_time: u64, // 统计结束时间（毫秒）

    #[serde(rename = "firstId")]
    pub first_id: u64, // 首笔成交ID

    #[serde(rename = "lastId")]
    pub last_id: u64, // 末笔成交ID

    #[serde(rename = "count")]
    pub count: u64, // 成交笔数
}

/// 币安用户数据流 Listen Key 响应
/// POST /api/v3/userDataStream 或 POST /fapi/v1/listenKey 的响应
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ListenKeyResponse {
    #[serde(rename = "listenKey")]
    pub listen_key: String,
}
