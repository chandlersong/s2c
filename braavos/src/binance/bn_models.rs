use crate::binance::bn_tools::unix_2_readable;
use crate::models::{Decimal, UnixTimeStamp};
use crate::tools;
use crate::tools::{string_to_float, unix_time};
use serde::{Deserialize, Serialize};
use std::fmt;

pub mod bin {
    use crate::binance::bn_models::{SpotDepthData, TradeRaw};

    include!(concat!(env!("OUT_DIR"), "/binance.rs"));

    impl From<TradeRaw> for Trade {
        fn from(value: TradeRaw) -> Self {
            Self {
                timestamp: value.event_time,
                symbol: value.symbol,
                trade_id: value.trade_id,
                price: value.price,
                quantity: value.quantity,
                trade_timestamp: value.trade_timestamp,
                is_marker: value.buyer_is_marker,
            }
        }
    }

    impl From<SpotDepthData> for SpotDepth {
        fn from(value: SpotDepthData) -> Self {
            let mut bids = vec![];
            for b in &value.bids {
                bids.push(SpotDepthLevel {
                    price: b.price,
                    quantity: b.quantity,
                });
            }

            let mut asks = vec![];
            for a in &value.asks {
                asks.push(SpotDepthLevel {
                    price: a.price,
                    quantity: a.quantity,
                });
            }

            Self {
                timestamp: value.event_time,
                symbol: value.symbol,
                first_update_id: value.first_update_id,
                final_update_id: value.final_update_id,
                bids,
                asks,
            }
        }
    }
}

pub const BINANCE_API_BASE: &str = "https://api.binance.com/";
pub const PING_PATH: &str = "/api/v3/ping";
pub const EXCHANGE_INFO_PATH: &str = "/api/v3/exchangeInfo";
pub const SERVER_TIME_PATH: &str = "/api/v3/time";
pub const SPOT_TICKER_API_PATH: &str = "/api/v3/ticker/price";

pub const PORTFOLIO_MARGIN_BASE: &str = "https://papi.binance.com/";
pub const BALANCE_PATH: &str = "/papi/v1/balance";
pub const SWAP_POSITION_PATH: &str = "/papi/v1/um/positionRisk";
pub const LISTEN_KEY_PATH: &str = "/papi/v1/listenKey";

pub const WS_SWAP_STREAM_URL_BASE: &str = "wss://fstream.binance.com/";
pub const WS_PING_COMMAND: &str = "ping";
pub const WS_TIME_COMMAND: &str = "time";
pub const WS_SUBSCRIBE_COMMAND: &str = "SUBSCRIBE";
pub const WS_SET_PROPERTY_COMMAND: &str = "SET_PROPERTY";
pub const WS_GET_PROPERTY_COMMAND: &str = "GET_PROPERTY";

#[derive(Serialize, Deserialize, Debug)]
pub enum SpotWsSubscribe {
    AllMiniTicker,
}

impl From<SpotWsSubscribe> for String {
    fn from(subscription: SpotWsSubscribe) -> Self {
        String::from(match subscription {
            SpotWsSubscribe::AllMiniTicker => String::from("!miniTicker@arr"),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PMBalance {
    pub asset: String,

    #[serde(rename = "totalWalletBalance")]
    pub total_wallet_balance: Decimal, // 钱包余额 =  全仓杠杆未锁定 + 全仓杠杆锁定 + u本位合约钱包余额 + 币本位合约钱包余额

    #[serde(rename = "crossMarginAsset")]
    pub cross_margin_asset: Decimal, // 全仓资产 = 全仓杠杆未锁定 + 全仓杠杆锁定

    #[serde(rename = "crossMarginBorrowed")]
    pub cross_margin_borrowed: Decimal, // 全仓杠杆借贷

    #[serde(rename = "crossMarginFree")]
    pub cross_margin_free: Decimal, // 全仓杠杆未锁定

    #[serde(rename = "crossMarginInterest")]
    pub cross_margin_interest: Decimal, // 全仓杠杆利息

    #[serde(rename = "crossMarginLocked")]
    pub cross_margin_locked: Decimal, //全仓杠杆锁定

    #[serde(rename = "umWalletBalance")]
    pub um_wallet_balance: Decimal, // u本位合约钱包余额

    #[serde(rename = "umUnrealizedPNL")]
    pub um_unrealized_pnl: Decimal, // u本位未实现盈亏

    #[serde(rename = "cmWalletBalance")]
    pub cm_wallet_balance: Decimal, // 币本位合约钱包余额

    #[serde(rename = "cmUnrealizedPNL")]
    pub cm_unrealized_pnl: Decimal, // 币本位未实现盈亏

    #[serde(rename = "updateTime")]
    pub update_time: UnixTimeStamp,

    #[serde(rename = "negativeBalance")]
    pub negative_balance: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UMSwapAssert {
    pub symbol: String, // 交易对

    #[serde(rename = "initialMargin")]
    pub initial_margin: Decimal, // 当前所需起始保证金(基于最新标记价格)

    #[serde(rename = "maintMargin")]
    pub maint_margin: Decimal, // 维持保证金

    #[serde(rename = "unrealizedProfit")]
    pub unrealized_profit: Decimal, // 持仓未实现盈亏

    #[serde(rename = "positionInitialMargin")]
    pub position_initial_margin: Decimal, //持仓所需起始保证金(基于最新标记价格)

    #[serde(rename = "openOrderInitialMargin")]
    pub open_order_initial_margin: Decimal, // 当前挂单所需起始保证金(基于最新标记价格)

    #[serde(rename = "leverage")]
    pub leverage: Decimal, // 杠杆倍率

    #[serde(rename = "entryPrice")]
    pub entry_price: Decimal, // 持仓成本价

    #[serde(rename = "maxNotional")]
    pub max_notional: Decimal, // 当前杠杆下用户可用的最大名义价值

    #[serde(rename = "bidNotional")]
    pub bid_notional: Decimal, // 买单净值，忽略

    #[serde(rename = "askNotional")]
    pub ask_notional: Decimal, // 卖单净值，忽略

    #[serde(rename = "positionSide")]
    pub position_side: String, // 持仓方向

    #[serde(rename = "positionAmt")]
    pub position_amt: Decimal, //  持仓数量

    #[serde(rename = "updateTime")]
    pub update_time: UnixTimeStamp, // 更新时间

    #[serde(rename = "breakEvenPrice")]
    pub break_even_price: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UMSwapPosition {
    #[serde(rename = "entryPrice")]
    pub entry_price: Decimal, // 开仓均价

    #[serde(rename = "leverage", deserialize_with = "tools::str_to_u16")]
    pub leverage: u16, // 当前杠杆倍数

    #[serde(rename = "markPrice")]
    pub mark_price: Decimal, // 当前标记价格

    #[serde(rename = "maxNotionalValue")]
    pub max_notional_value: Decimal, // 当前杠杆倍数允许的名义价值上限

    #[serde(rename = "positionAmt")]
    pub position_amt: Decimal, // 头寸数量，符号代表多空方向, 正数为多，负数为空

    #[serde(rename = "notional")]
    pub notional: Decimal, // 名义价值

    #[serde(rename = "symbol")]
    pub symbol: String, // 交易对

    #[serde(rename = "unRealizedProfit")]
    pub unrealized_profit: Decimal, // 持仓未实现盈亏

    #[serde(rename = "liquidationPrice")]
    pub liquidation_price: Decimal, // 爆仓价格

    #[serde(rename = "positionSide")]
    pub position_side: String, // 持仓方向

    #[serde(rename = "updateTime")]
    pub update_time: UnixTimeStamp, // 更新时间

    #[serde(rename = "breakEvenPrice")]
    pub break_even_price: Decimal, //表仓位盈亏平衡价
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerTime {
    #[serde(rename = "serverTime")]
    pub time: UnixTimeStamp,
}

#[derive(Deserialize, Debug)]
pub struct ExchangeInfo {
    #[serde(rename = "timezone")]
    pub timezone: String,

    #[serde(rename = "serverTime")]
    pub server_time: u64,

    #[serde(rename = "rateLimits")]
    pub rate_limits: Vec<RateLimit>,

    #[serde(rename = "exchangeFilters")]
    pub exchange_filters: Vec<ExchangeFilter>,

    #[serde(rename = "symbols")]
    pub symbols: Vec<ExchangeSymbol>,
}

#[derive(Deserialize, Debug)]
pub struct RateLimit {
    #[serde(rename = "rateLimitType")]
    pub rate_limit_type: String,

    #[serde(rename = "interval")]
    pub interval: String,

    #[serde(rename = "intervalNum")]
    pub interval_num: i32,

    #[serde(rename = "limit")]
    pub limit: i32,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "filterType")]
pub enum ExchangeFilter {
    #[serde(rename = "PRICE_FILTER")]
    PriceFilter {
        #[serde(rename = "minPrice", default)]
        min_price: Option<String>,
        #[serde(rename = "maxPrice", default)]
        max_price: Option<String>,
        #[serde(rename = "tickSize", default)]
        tick_size: Option<String>,
    },
    #[serde(rename = "LOT_SIZE")]
    LotSize {
        #[serde(rename = "minQty", default)]
        min_qty: Option<String>,
        #[serde(rename = "maxQty", default)]
        max_qty: Option<String>,
        #[serde(rename = "stepSize", default)]
        step_size: Option<String>,
    },
    #[serde(other)]
    Unknown,
}
#[derive(Deserialize, Debug)]
pub struct ExchangeSymbol {
    #[serde(rename = "symbol")]
    pub symbol: String,
    #[serde(rename = "status")]
    pub status: String,
    #[serde(rename = "baseAsset")]
    pub base_asset: String,
    #[serde(rename = "baseAssetPrecision")]
    pub base_asset_precision: i32,
    #[serde(rename = "quoteAsset")]
    pub quote_asset: String,
    #[serde(rename = "quotePrecision")]
    pub quote_precision: i32,
    #[serde(rename = "quoteAssetPrecision")]
    pub quote_asset_precision: i32,
    #[serde(rename = "baseCommissionPrecision")]
    pub base_commission_precision: i32,
    #[serde(rename = "quoteCommissionPrecision")]
    pub quote_commission_precision: i32,
    #[serde(rename = "orderTypes")]
    pub order_types: Vec<String>,
    #[serde(rename = "icebergAllowed")]
    pub iceberg_allowed: bool,
    #[serde(rename = "ocoAllowed")]
    pub oco_allowed: bool,
    #[serde(rename = "quoteOrderQtyMarketAllowed")]
    pub quote_order_qty_market_allowed: bool,
    #[serde(rename = "allowTrailingStop")]
    pub allow_trailing_stop: bool,
    #[serde(rename = "cancelReplaceAllowed")]
    pub cancel_replace_allowed: bool,
    #[serde(rename = "isSpotTradingAllowed")]
    pub is_spot_trading_allowed: bool,
    #[serde(rename = "isMarginTradingAllowed")]
    pub is_margin_trading_allowed: bool,
    pub filters: Vec<ExchangeFilter>,
    pub permissions: Vec<String>,
    #[serde(rename = "defaultSelfTradePreventionMode")]
    pub default_self_trade_prevention_mode: String,
    #[serde(rename = "allowedSelfTradePreventionModes")]
    pub allowed_self_trade_prevention_modes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UMSwapBalance {
    #[serde(rename = "tradeGroupId")]
    pub trade_group_id: i32,
    #[serde(rename = "assets")]
    pub assets: Vec<UMSwapAssert>,
    #[serde(rename = "positions")]
    pub positions: Vec<UMSwapAssert>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticker {
    #[serde(rename = "symbol")]
    pub symbol: String, // 交易对
    #[serde(rename = "price")]
    pub price: Decimal, // 价格
    #[serde(rename = "time")]
    pub time: Option<UnixTimeStamp>, // 撮合引擎时间,Spot的不存在这个数据
}

#[derive(Clone)]
pub struct SecurityInfo {
    pub api_key: String,
    pub api_secret: String,
}

pub struct TimeStampRequest {
    pub timestamp: UnixTimeStamp,
    pub rec_window: u16,
}

impl fmt::Display for TimeStampRequest {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "timestamp={}&recvWindow={}",
            self.timestamp, self.rec_window
        )
    }
}

impl Default for TimeStampRequest {
    fn default() -> Self {
        TimeStampRequest {
            timestamp: unix_time(),
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ListenKeyResponse {
    #[serde(rename = "listenKey")]
    pub listen_key: String,
}

#[derive(PartialEq, Debug, Serialize, Deserialize, Clone)]
pub struct Bids {
    #[serde(with = "string_to_float")]
    pub price: f64,
    #[serde(with = "string_to_float")]
    pub quantity: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Asks {
    #[serde(with = "string_to_float")]
    pub price: f64,
    #[serde(with = "string_to_float")]
    pub quantity: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SpotDepthData {
    #[serde(rename = "e")]
    pub event_type: String, // 事件类型：depthUpdate

    #[serde(rename = "E")]
    pub event_time: u64,

    #[serde(rename = "s")]
    pub symbol: String,

    #[serde(rename = "U")]
    pub first_update_id: u64,

    #[serde(rename = "u")]
    pub final_update_id: u64,

    #[serde(rename = "pu")]
    #[serde(default)]
    pub previous_final_update_id: Option<u64>,

    #[serde(rename = "b")]
    pub bids: Vec<Bids>,

    #[serde(rename = "a")]
    pub asks: Vec<Asks>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StreamAllMiniTickerResponse {
    #[serde(rename = "stream")]
    pub stream: String, // 事件类型：!miniTicker@arr

    #[serde(rename = "data")]
    pub tickers: Vec<MiniTicker>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct MiniTicker {
    #[serde(rename = "e")]
    pub event_type: String, // 事件类型：24hrMiniTicker

    #[serde(rename = "E")]
    pub event_time: u64,
    //
    #[serde(rename = "s")]
    pub symbol: String,
    //
    #[serde(rename = "c")]
    #[serde(with = "string_to_float")]
    pub close: f64,

    #[serde(rename = "o")]
    #[serde(with = "string_to_float")]
    pub open: f64,

    #[serde(rename = "h")]
    #[serde(with = "string_to_float")]
    pub high: f64,

    #[serde(rename = "l")]
    #[serde(with = "string_to_float")]
    pub low: f64,

    #[serde(rename = "v")]
    #[serde(with = "string_to_float")]
    pub volume: f64,

    #[serde(rename = "q")]
    #[serde(with = "string_to_float")]
    pub quote_volume: f64,
}

impl fmt::Display for MiniTicker {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "MiniTicker: symbol:{},time:{},close:{}",
            self.symbol,
            unix_2_readable(&self.event_time),
            self.close
        )
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TradeRaw {
    #[serde(rename = "E")]
    pub event_time: u64,

    #[serde(rename = "s")]
    pub symbol: String,

    #[serde(rename = "t")]
    pub trade_id: u64,

    #[serde(rename = "p")]
    #[serde(with = "string_to_float")]
    pub price: f64,

    #[serde(rename = "q")]
    #[serde(with = "string_to_float")]
    pub quantity: f64,

    #[serde(rename = "T")]
    pub trade_timestamp: u64,

    #[serde(rename = "m")]
    pub buyer_is_marker: bool, //买方是否是做市方。如true，则此次成交是一个主动卖出单，否则是一个主动买入单。
}
