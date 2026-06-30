use crate::models::Decimal;
use crate::tools::string_to_decimal;
use crate::tools::string_to_option_decimal;
use li::errors::LiError;
use li::websocket::models::WebSocketMessage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BinanceSwapWebSocketStreamWrapper {
    pub stream: String,
    pub data: BinanceSwapWebSocketStreamResponse,
}

impl WebSocketMessage for BinanceSwapWebSocketStreamWrapper {
    fn from_text(text: &str) -> Result<Self, LiError> {
        serde_json::from_str(text).map_err(|e| LiError::from(e))
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum BinanceSwapWebSocketStreamResponse {
    AggTrade(AggTradePayload),
    MarkPrice(MarkPricePayload),
    AllMarketPrice(Vec<MarkPricePayload>),
    Kline(SwapWebsocketKlinePayload),
    ContinuousKline(ContinuousKlinePayload),
    MiniTicker(MiniTickerPayload),
    AllMarketMiniTicker(Vec<MiniTickerPayload>),
    Ticker(TickerPayload),
    ALLMarketTicker(Vec<TickerPayload>),
    BookTicker(BookTickerPayload),
    ForceOrder(ForceOrderPayload),
    AllMarketForceOrder(AllMarketForceOrderPayload),
    Depth(DepthPayload),
    DepthUpdate(DepthUpdatePayload),
    RpiDepth(RpiDepthPayload),
    CompositeIndex(CompositeIndexPayload),
    ContractInfo(ContractInfoPayload),
    AssetIndex(AssetIndexPayload),
    TradingSession(TradingSessionPayload),
}

impl WebSocketMessage for BinanceSwapWebSocketStreamResponse {
    fn from_text(text: &str) -> Result<Self, LiError> {
        serde_json::from_str(text).map_err(|e| LiError::from(e))
    }
}

///
///  [归集交易]https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/websocket-market-streams/Aggregate-Trade-Streams)
///
/// ```json
/// {
///   "e": "aggTrade",  // 事件类型
///   "E": 123456789,   // 事件时间
///   "s": "BNBUSDT",   // 交易对
///   "a": 5933014,		  // 归集成交 ID
///   "p": "0.001",     // 成交价格
///   "q": "100",       // 成交量，包含RPI订单数据
///   "nq": "100",      // 普通订单成交量，不包含RPI订单数据
///   "f": 100,         // 被归集的首个交易ID
///   "l": 105,         // 被归集的末次交易ID
///   "T": 123456785,   // 成交时间
///   "m": true         // 买方是否是做市方。如true，则此次成交是一个主动卖出单，否则是一个主动买入单。
/// }
/// ```
///
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AggTradePayload {
    /// 事件类型
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// 归集成交 ID
    #[serde(rename = "a")]
    pub agg_trade_id: u64,

    /// 成交价格（字符串数值）
    #[serde(rename = "p", with = "string_to_decimal")]
    pub price: Decimal,

    /// 成交量，包含RPI订单数据（字符串数值）
    #[serde(rename = "q", with = "string_to_decimal")]
    pub quantity: Decimal,

    /// 普通订单成交量，不包含RPI订单数据（字符串数值）
    #[serde(rename = "nq", with = "string_to_decimal")]
    pub normal_quantity: Decimal,

    /// 被归集的首个交易ID
    #[serde(rename = "f")]
    pub first_trade_id: u64,

    /// 被归集的末次交易ID
    #[serde(rename = "l")]
    pub last_trade_id: u64,

    /// 成交时间
    #[serde(rename = "T")]
    pub trade_time: u64,

    /// 买方是否是做市方
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
}

///
///  [标记价格](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/websocket-market-streams/Mark-Price-Stream)
/// ```json
///    {
///     "e": "markPriceUpdate",  	// 事件类型
///     "E": 1562305380000,      	// 事件时间
///     "s": "BTCUSDT",          	// 交易对
///     "p": "11794.15000000",   	// 标记价格
///     "ap": "11794.15000000",   // 标记价格移动平均
///    "i": "11784.62659091",		// 现货指数价格
///     "P": "11784.25641265",		// 预估结算价,仅在结算前最后一小时有参考价值
///     "r": "0.00038167",       	// 资金费率
///     "T": 1562306400000       	// 下次资金时间
///   }
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MarkPricePayload {
    /// 事件类型
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// 标记价格（字符串数值）
    #[serde(rename = "p", with = "string_to_decimal")]
    pub mark_price: Decimal,

    /// 标记价格移动平均（字符串数值）
    #[serde(rename = "ap", with = "string_to_decimal")]
    pub avg_price: Decimal,

    /// 现货指数价格（字符串数值）
    #[serde(rename = "i", with = "string_to_decimal")]
    pub index_price: Decimal,

    /// 预估结算价（字符串数值）
    #[serde(rename = "P", with = "string_to_decimal")]
    pub settlement_price: Decimal,

    /// 资金费率（字符串数值）
    #[serde(rename = "r", with = "string_to_decimal")]
    pub funding_rate: Decimal,

    /// 下次资金时间
    #[serde(rename = "T")]
    pub next_funding_time: u64,
}

///
/// [K线数据](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/websocket-market-streams/Kline-Candlestick-Streams)
///
/// ```json
/// {
///   "e": "kline",     // 事件类型
///   "E": 123456789,   // 事件时间
///   "s": "BNBUSDT",    // 交易对
///   "k": {
///     "t": 123400000, // 这根K线的起始时间
///     "T": 123460000, // 这根K线的结束时间
///     "s": "BNBUSDT",  // 交易对
///     "i": "1m",      // K线间隔
///     "f": 100,       // 这根K线期间第一笔成交ID
///     "L": 200,       // 这根K线期间末一笔成交ID
///     "o": "0.0010",  // 这根K线期间第一笔成交价
///     "c": "0.0020",  // 这根K线期间末一笔成交价
///     "h": "0.0025",  // 这根K线期间最高成交价
///     "l": "0.0015",  // 这根K线期间最低成交价
///     "v": "1000",    // 这根K线期间成交量
///     "n": 100,       // 这根K线期间成交笔数
///     "x": false,     // 这根K线是否完结(是否已经开始下一根K线)
///     "q": "1.0000",  // 这根K线期间成交额
///     "V": "500",     // 主动买入的成交量
///     "Q": "0.500",   // 主动买入的成交额
///     "B": "123456"   // 忽略此参数
///   }
/// }
/// ```
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SwapWebsocketKlinePayload {
    /// 事件类型
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// K线数据
    #[serde(rename = "k")]
    pub kline: SwapWebsocketKlineData,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SwapWebsocketKlineData {
    /// 这根K线的起始时间
    #[serde(rename = "t")]
    pub start_time: u64,

    /// 这根K线的结束时间
    #[serde(rename = "T")]
    pub end_time: u64,

    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// K线间隔
    #[serde(rename = "i")]
    pub interval: String,

    /// 这根K线期间第一笔成交ID
    #[serde(rename = "f")]
    pub first_trade_id: u64,

    /// 这根K线期间末一笔成交ID
    #[serde(rename = "L")]
    pub last_trade_id: u64,

    /// 这根K线期间第一笔成交价（字符串数值）
    #[serde(rename = "o", with = "string_to_decimal")]
    pub open: Decimal,

    /// 这根K线期间末一笔成交价（字符串数值）
    #[serde(rename = "c", with = "string_to_decimal")]
    pub close: Decimal,

    /// 这根K线期间最高成交价（字符串数值）
    #[serde(rename = "h", with = "string_to_decimal")]
    pub high: Decimal,

    /// 这根K线期间最低成交价（字符串数值）
    #[serde(rename = "l", with = "string_to_decimal")]
    pub low: Decimal,

    /// 这根K线期间成交量（字符串数值）
    #[serde(rename = "v", with = "string_to_decimal")]
    pub volume: Decimal,

    /// 这根K线期间成交笔数
    #[serde(rename = "n")]
    pub number_of_trades: u64,

    /// 这根K线是否完结
    #[serde(rename = "x")]
    pub is_close: bool,

    /// 这根K线期间成交额（字符串数值）
    #[serde(rename = "q", with = "string_to_decimal")]
    pub quote_asset_volume: Decimal,

    /// 主动买入的成交量（字符串数值）
    #[serde(rename = "V", with = "string_to_decimal")]
    pub taker_buy_base_asset_volume: Decimal,

    /// 主动买入的成交额（字符串数值）
    #[serde(rename = "Q", with = "string_to_decimal")]
    pub taker_buy_quote_asset_volume: Decimal,

    /// 忽略此参数
    #[serde(rename = "B", with = "string_to_option_decimal")]
    pub ignore: Option<Decimal>,
}

///
///  [连续K线数据](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/websocket-market-streams/Continuous-Kline-Candlestick-Streams)
///
/// ```json
/// {
///   "e":"continuous_kline",	// 事件类型
///   "E":1607443058651,		// 事件时间
///   "ps":"BTCUSDT",			// 标的交易对
///   "ct":"PERPETUAL",			// 合约类型
///   "k":{
///     "t":1607443020000,		// 这根K线的起始时间
///     "T":1607443079999,		// 这根K线的结束时间
///     "i":"1m",				// K线间隔
///     "f":116467658886,		// 这根K线期间第一笔更新ID
///     "L":116468012423,		// 这根K线期间末一笔更新ID
///     "o":"18787.00",			// 这根K线期间第一笔成交价
///     "c":"18804.04",			// 这根K线期间末一笔成交价
///     "h":"18804.04",			// 这根K线期间最高成交价
///     "l":"18786.54",			// 这根K线期间最低成交价
///     "v":"197.664",			// 这根K线期间成交量
///     "n":543,				// 这根K线期间成交笔数
///     "x":false,				// 这根K线是否完结(是否已经开始下一根K线)
///     "q":"3715253.19494",	// 这根K线期间成交额
///     "V":"184.769",			// 主动买入的成交量
///     "Q":"3472925.84746",	// 主动买入的成交额
///     "B":"0"					// 忽略此参数
///   }
/// }
/// ```
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ContinuousKlinePayload {
    /// 事件类型
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 标的交易对
    #[serde(rename = "ps")]
    pub pair: String,

    /// 合约类型
    #[serde(rename = "ct")]
    pub contract_type: String,

    /// K线数据
    #[serde(rename = "k")]
    pub kline: ContinuousKlineData,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ContinuousKlineData {
    /// 这根K线的起始时间
    #[serde(rename = "t")]
    pub start_time: u64,

    /// 这根K线的结束时间
    #[serde(rename = "T")]
    pub end_time: u64,

    /// K线间隔
    #[serde(rename = "i")]
    pub interval: String,

    /// 这根K线期间第一笔更新ID
    #[serde(rename = "f")]
    pub first_update_id: u64,

    /// 这根K线期间末一笔更新ID
    #[serde(rename = "L")]
    pub last_update_id: u64,

    /// 这根K线期间第一笔成交价（字符串数值）
    #[serde(rename = "o", with = "string_to_decimal")]
    pub open: Decimal,

    /// 这根K线期间末一笔成交价（字符串数值）
    #[serde(rename = "c", with = "string_to_decimal")]
    pub close: Decimal,

    /// 这根K线期间最高成交价（字符串数值）
    #[serde(rename = "h", with = "string_to_decimal")]
    pub high: Decimal,

    /// 这根K线期间最低成交价（字符串数值）
    #[serde(rename = "l", with = "string_to_decimal")]
    pub low: Decimal,

    /// 这根K线期间成交量（字符串数值）
    #[serde(rename = "v", with = "string_to_decimal")]
    pub base_asset_volume: Decimal,

    /// 这根K线期间成交笔数
    #[serde(rename = "n")]
    pub trade_count: u64,

    /// 这根K线是否完结
    #[serde(rename = "x")]
    pub is_final: bool,

    /// 这根K线期间成交额（字符串数值）
    #[serde(rename = "q", with = "string_to_decimal")]
    pub quote_asset_volume: Decimal,

    /// 主动买入的成交量（字符串数值）
    #[serde(rename = "V", with = "string_to_decimal")]
    pub active_buy_base_asset_volume: Decimal,

    /// 主动买入的成交额（字符串数值）
    #[serde(rename = "Q", with = "string_to_decimal")]
    pub active_buy_quote_asset_volume: Decimal,

    /// 忽略此参数
    #[serde(rename = "B", with = "string_to_option_decimal")]
    pub ignore: Option<Decimal>,
}

///
/// [按交易对的精简Ticker](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/websocket-market-streams/Individual-Symbol-Mini-Ticker-Stream)
///
/// ```json
///   {
///     "e": "24hrMiniTicker",  // 事件类型
///     "E": 123456789,         // 事件时间(毫秒)
///     "s": "BNBUSDT",          // 交易对
///     "c": "0.0025",          // 最新成交价格
///     "o": "0.0010",          // 24小时前开始第一笔成交价格
///     "h": "0.0025",          // 24小时内最高成交价
///     "l": "0.0010",          // 24小时内最低成交价
///     "v": "10000",           // 成交量
///     "q": "18"               // 成交额
///   }
/// ```
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MiniTickerPayload {
    /// 事件类型
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间(毫秒)
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// 最新成交价格（字符串数值）
    #[serde(rename = "c", with = "string_to_decimal")]
    pub close_price: Decimal,

    /// 24小时前开始第一笔成交价格（字符串数值）
    #[serde(rename = "o", with = "string_to_decimal")]
    pub open_price: Decimal,

    /// 24小时内最高成交价（字符串数值）
    #[serde(rename = "h", with = "string_to_decimal")]
    pub high_price: Decimal,

    /// 24小时内最低成交价（字符串数值）
    #[serde(rename = "l", with = "string_to_decimal")]
    pub low_price: Decimal,

    /// 成交量（字符串数值）
    #[serde(rename = "v", with = "string_to_decimal")]
    pub base_asset_volume: Decimal,

    /// 成交额（字符串数值）
    #[serde(rename = "q", with = "string_to_decimal")]
    pub quote_asset_volume: Decimal,
}

///
/// [按Symbol的完整Ticker](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/websocket-market-streams/Individual-Symbol-Ticker-Streams)
///
/// ```json
/// {
///   "e": "24hrTicker",  // 事件类型
///   "E": 123456789,     // 事件时间
///   "s": "BNBUSDT",      // 交易对
///   "p": "0.0015",      // 24小时价格变化
///   "P": "250.00",      // 24小时价格变化(百分比)
///   "w": "0.0018",      // 平均价格
///   "c": "0.0025",      // 最新成交价格
///   "Q": "10",          // 最新成交价格上的成交量
///   "o": "0.0010",      // 24小时内第一比成交的价格
///   "h": "0.0025",      // 24小时内最高成交价
///   "l": "0.0010",      // 24小时内最低成交价
///   "v": "10000",       // 24小时内成交量
///   "q": "18",          // 24小时内成交额
///   "O": 0,             // 统计开始时间
///   "C": 86400000,      // 统计关闭时间
///   "F": 0,             // 24小时内第一笔成交交易ID
///   "L": 18150,         // 24小时内最后一笔成交交易ID
///   "n": 18151          // 24小时内成交数
/// }
/// ```
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TickerPayload {
    /// 事件类型
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// 24小时价格变化（字符串数值）
    #[serde(rename = "p", with = "string_to_decimal")]
    pub price_change: Decimal,

    /// 24小时价格变化(百分比)（字符串数值）
    #[serde(rename = "P", with = "string_to_decimal")]
    pub price_change_percent: Decimal,

    /// 平均价格（字符串数值）
    #[serde(rename = "w", with = "string_to_decimal")]
    pub weighted_avg_price: Decimal,

    /// 最新成交价格（字符串数值）
    #[serde(rename = "c", with = "string_to_decimal")]
    pub last_price: Decimal,

    /// 最新成交价格上的成交量（字符串数值）
    #[serde(rename = "Q", with = "string_to_decimal")]
    pub last_qty: Decimal,

    /// 24小时内第一比成交的价格（字符串数值）
    #[serde(rename = "o", with = "string_to_decimal")]
    pub open_price: Decimal,

    /// 24小时内最高成交价（字符串数值）
    #[serde(rename = "h", with = "string_to_decimal")]
    pub high_price: Decimal,

    /// 24小时内最低成交价（字符串数值）
    #[serde(rename = "l", with = "string_to_decimal")]
    pub low_price: Decimal,

    /// 24小时内成交量（字符串数值）
    #[serde(rename = "v", with = "string_to_decimal")]
    pub base_asset_volume: Decimal,

    /// 24小时内成交额（字符串数值）
    #[serde(rename = "q", with = "string_to_decimal")]
    pub quote_asset_volume: Decimal,

    /// 统计开始时间
    #[serde(rename = "O")]
    pub open_time: u64,

    /// 统计关闭时间
    #[serde(rename = "C")]
    pub close_time: u64,

    /// 24小时内第一笔成交交易ID
    #[serde(rename = "F")]
    pub first_trade_id: u64,

    /// 24小时内最后一笔成交交易ID
    #[serde(rename = "L")]
    pub last_trade_id: u64,

    /// 24小时内成交数
    #[serde(rename = "n")]
    pub trade_count: u64,
}

///
/// [全市场最优挂单信息](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/websocket-market-streams/All-Book-Tickers-Stream)
/// ```json
///{
///  "e":"bookTicker",		// 事件类型
///   "u":400900217,     	// 更新ID
///   "E": 1568014460893,	// 事件推送时间
///   "T": 1568014460891,	// 撮合时间
///   "s":"BNBUSDT",     	// 交易对
///   "b":"25.35190000", 	// 买单最优挂单价格
///   "B":"31.21000000", 	// 买单最优挂单数量
///   "a":"25.36520000", 	// 卖单最优挂单价格
///   "A":"40.66000000"  	// 卖单最优挂单数量
/// }
/// ```
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BookTickerPayload {
    /// 事件类型
    #[serde(rename = "e")]
    pub event: String,

    /// 更新ID
    #[serde(rename = "u")]
    pub update_id: u64,

    /// 事件推送时间
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 撮合时间
    #[serde(rename = "T")]
    pub trade_time: u64,

    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// 买单最优挂单价格（字符串数值）
    #[serde(rename = "b", with = "string_to_decimal")]
    pub best_bid_price: Decimal,

    /// 买单最优挂单数量（字符串数值）
    #[serde(rename = "B", with = "string_to_decimal")]
    pub best_bid_qty: Decimal,

    /// 卖单最优挂单价格（字符串数值）
    #[serde(rename = "a", with = "string_to_decimal")]
    pub best_ask_price: Decimal,

    /// 卖单最优挂单数量（字符串数值）
    #[serde(rename = "A", with = "string_to_decimal")]
    pub best_ask_qty: Decimal,
}

///
/// [强平订单](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/websocket-market-streams/Liquidation-Order-Streams)
/// 推送特定symbol的强平订单快照信息。 1000ms内至多仅推送**一条最大的强平**订单作为快照
///
///
/// ```json
/// {
///
/// 	"e":"forceOrder",                   // 事件类型
/// 	"E":1568014460893,                  // 事件时间
/// 	"o":{
/// 		"s":"BTCUSDT",                   // 交易对
/// 		"S":"SELL",                      // 订单方向
/// 		"o":"LIMIT",                     // 订单类型
/// 		"f":"IOC",                       // 有效方式
/// 		"q":"0.014",                     // 订单数量
/// 		"p":"9910",                      // 订单价格
/// 		"ap":"9910",                     // 平均价格
/// 		"X":"FILLED",                    // 订单状态
/// 		"l":"0.014",                     // 订单最近成交量
/// 		"z":"0.014",                     // 订单累计成交量
/// 		"T":1568014460893,          	 // 交易时间
/// 	}
/// }
/// ```
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ForceOrderPayload {
    /// 事件类型
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 订单信息
    #[serde(rename = "o")]
    pub order: Option<ForceOrderInfo>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ForceOrderInfo {
    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// 订单方向
    #[serde(rename = "S")]
    pub side: String,

    /// 订单类型
    #[serde(rename = "o")]
    pub order_type: String,

    /// 有效方式
    #[serde(rename = "f")]
    pub time_in_force: String,

    /// 订单数量（字符串数值）
    #[serde(rename = "q", with = "string_to_decimal")]
    pub quantity: Decimal,

    /// 订单价格（字符串数值）
    #[serde(rename = "p", with = "string_to_decimal")]
    pub price: Decimal,

    /// 平均价格（字符串数值）
    #[serde(rename = "ap", with = "string_to_decimal")]
    pub avg_price: Decimal,

    /// 订单状态
    #[serde(rename = "X")]
    pub order_status: String,

    /// 订单最近成交量（字符串数值）
    #[serde(rename = "l", with = "string_to_decimal")]
    pub last_filled_qty: Decimal,

    /// 订单累计成交量（字符串数值）
    #[serde(rename = "z", with = "string_to_decimal")]
    pub executed_qty: Decimal,

    /// 交易时间
    #[serde(rename = "T")]
    pub trade_time: u64,
}

///
/// [全市场强平订单](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/websocket-market-streams/All-Market-Liquidation-Order-Streams)
/// 推送全市场强平订单快照信息 每个symbol，1000ms内至多仅推送**一条最大的强平订单**作为快照
///
/// ```json
///{
/// 	"e":"forceOrder",                   // 事件类型
/// 	"E":1568014460893,                  // 事件时间
/// 	"o":{
/// 		"s":"BTCUSDT",                   // 交易对
/// 		"S":"SELL",                      // 订单方向
/// 		"o":"LIMIT",                     // 订单类型
/// 		"f":"IOC",                       // 有效方式
/// 		"q":"0.014",                     // 订单数量
/// 		"p":"9910",                      // 订单价格
/// 		"ap":"9910",                     // 平均价格
/// 		"X":"FILLED",                    // 订单状态
/// 		"l":"0.014",                     // 订单最近成交量
/// 		"z":"0.014",                     // 订单累计成交量
/// 		"T":1568014460893,          	 // 交易时间
///	}
/// }
/// ```
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AllMarketForceOrderPayload {
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间
    #[serde(rename = "E")]
    pub event_time: u64,

    #[serde(rename = "o")]
    pub force_order: ForceOrderPayload,
}

///
/// [有限档深度信息](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/websocket-market-streams/Partial-Book-Depth-Streams)
/// 推送有限档深度信息。levels表示几档买卖单信息, 可选 5/10/20档
///
/// ```json
/// {
///   "e": "depthUpdate", 			// 事件类型
///   "E": 1571889248277, 			// 事件时间
///   "T": 1571889248276, 			// 交易时间
///   "s": "BTCUSDT",
///   "U": 390497796,           // 从上次推送至今新增的第一个 update Id
///   "u": 390497878,           // 从上次推送至今新增的最后一个 update Id
///   "pu": 390497794,          // 上次推送的最后一个update Id(即上条消息的‘u’)
///   "b": [             				// 买方
///     [
///       "7403.89",  	  			// 价格
///       "0.002"     		  		// 数量
///     ],
///     [
///       "7403.90",
///       "3.906"
///     ],
///     [
///       "7404.00",
///       "1.428"
///     ],
///     [
///       "7404.85",
///       "5.239"
///    ],
///     [
///       "7405.43",
///       "2.562"
///     ]
///   ],
///   "a": [          				// 卖方
///     [
///       "7405.96",  				// 价格
///       "3.340"     				// 数量
///     ],
///     [
///       "7406.63",
///       "4.525"
///     ],
///     [
///      "7407.08",
///       "2.475"
///     ],
///    [
///       "7407.15",
///       "4.800"
///     ],
///     [
///       "7407.20",
///       "0.175"
///     ]
///   ]
/// }
/// ```
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DepthPayload {
    /// 事件类型
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 交易时间
    #[serde(rename = "T")]
    pub trade_time: u64,

    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// 从上次推送至今新增的第一个 update Id
    #[serde(rename = "U")]
    pub first_update_id: u64,

    /// 从上次推送至今新增的最后一个 update Id
    #[serde(rename = "u")]
    pub last_update_id: u64,

    /// 上次推送的最后一个update Id
    #[serde(rename = "pu")]
    pub prev_last_update_id: u64,

    /// 买方信息
    #[serde(rename = "b")]
    pub bids: Vec<[String; 2]>,

    /// 卖方信息
    #[serde(rename = "a")]
    pub asks: Vec<[String; 2]>,
}

///
/// [增量深度信息](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/websocket-market-streams/Diff-Book-Depth-Streams)
///
/// ```json
/// {
///   "e": "depthUpdate", 	// 事件类型
///   "E": 123456789,     	// 事件时间
///   "T": 123456788,     	// 撮合时间
///   "s": "BNBUSDT",      	// 交易对
///   "U": 157,           	// 从上次推送至今新增的第一个 update Id
///   "u": 160,           	// 从上次推送至今新增的最后一个 update Id
///   "pu": 149,          	// 上次推送的最后一个update Id(即上条消息的‘u’)
///   "b": [              	// 变动的买单深度
///     [
///       "0.0024",       	// 价格
///       "10"           	// 数量
///     ]
///   ],
///   "a": [              	// 变动的卖单深度
///     [
///       "0.0026",       	// 价格
///       "100"          	// 数量
///     ]
///   ]
/// }
/// ```
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DepthUpdatePayload {
    /// 事件类型
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 撮合时间
    #[serde(rename = "T")]
    pub trade_time: u64,

    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// 从上次推送至今新增的第一个 update Id
    #[serde(rename = "U")]
    pub first_update_id: u64,

    /// 从上次推送至今新增的最后一个 update Id
    #[serde(rename = "u")]
    pub last_update_id: u64,

    /// 上次推送的最后一个update Id
    #[serde(rename = "pu")]
    pub prev_last_update_id: u64,

    /// 变动的买单深度
    #[serde(rename = "b")]
    pub bids: Vec<[String; 2]>,

    /// 变动的卖单深度
    #[serde(rename = "a")]
    pub asks: Vec<[String; 2]>,
}

///
/// [RPI增量深度信息](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/websocket-market-streams/Diff-Book-Depth-Streams-RPI)
///
/// ```json
/// {
///   "e": "depthUpdate", 	// 事件类型
///   "E": 123456789,     	// 事件时间
///   "T": 123456788,     	// 撮合时间
///   "s": "BNBUSDT",      	// 交易对
///   "U": 157,           	// 从上次推送至今新增的第一个 update Id
///   "u": 160,           	// 从上次推送至今新增的最后一个 update Id
///   "pu": 149,          	// 上次推送的最后一个update Id(即上条消息的‘u’)
///   "b": [              	// 变动的买单深度
///     [
///      "0.0024",       	// 价格
///       "10"           	// 数量
///     ]
///   ],
///   "a": [              	// 变动的卖单深度
///     [
///       "0.0026",       	// 价格
///       "100"          	// 数量
///     ]
///   ]
/// }
/// ```
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RpiDepthPayload {
    /// 事件类型
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 撮合时间
    #[serde(rename = "T")]
    pub trade_time: u64,

    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// 从上次推送至今新增的第一个 update Id
    #[serde(rename = "U")]
    pub first_update_id: u64,

    /// 从上次推送至今新增的最后一个 update Id
    #[serde(rename = "u")]
    pub last_update_id: u64,

    /// 上次推送的最后一个update Id
    #[serde(rename = "pu")]
    pub prev_last_update_id: u64,

    /// 变动的买单深度
    #[serde(rename = "b")]
    pub bids: Vec<[String; 2]>,

    /// 变动的卖单深度
    #[serde(rename = "a")]
    pub asks: Vec<[String; 2]>,
}

///
/// [综合指数交易对信息流](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/websocket-market-streams/Composite-Index-Symbol-Information-Streams)
///
/// ```json
/// {
///   "e":"compositeIndex",		// 事件类型
///   "E":1602310596000,		// 事件事件
///   "s":"DEFIUSDT",			// 交易对
///   "p":"554.41604065",		// 价格
///   "C":"baseAsset",
///   "c":[					// 成分信息
///   	{
///   		"b":"BAL",			// 基础资产
///   		"q":"USDT",         // 报价资产
///   		"w":"1.04884844",	// 权重(数量)
///   		"W":"0.01457800",   // 权重(比例)
///   		"i":"24.33521021"   // 指数价格
///   	},
///   	{
///   		"b":"BAND",
///   		"q":"USDT",
///   		"w":"3.53782729",
///   		"W":"0.03935200",
///   		"i":"7.26420084"
///     }
///   ]
/// }
/// ```
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CompositeIndexPayload {
    /// 事件类型
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// 价格（字符串数值）
    #[serde(rename = "p", with = "string_to_decimal")]
    pub price: Decimal,

    /// 类型
    #[serde(rename = "C")]
    pub composition_type: Option<String>,

    /// 成分信息
    #[serde(rename = "c")]
    pub components: Option<Vec<CompositeComponent>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CompositeComponent {
    /// 基础资产
    #[serde(rename = "b")]
    pub base_asset: String,

    /// 报价资产
    #[serde(rename = "q")]
    pub quote_asset: String,

    /// 权重(数量)（字符串数值）
    #[serde(rename = "w", with = "string_to_decimal")]
    pub weight: Decimal,

    /// 权重(比例)（字符串数值）
    #[serde(rename = "W", with = "string_to_decimal")]
    pub weight_percent: Decimal,

    /// 指数价格（字符串数值）
    #[serde(rename = "i", with = "string_to_decimal")]
    pub index_price: Decimal,
}

///
/// [交易对信息信息流](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/websocket-market-streams/Contract-Info-Stream)
///
/// ```json
/// {
///     "e":"contractInfo",      // 事件类型
///     "E":1669356423908,       // 事件时间
///     "s":"IOTAUSDT",          // 交易对
///     "ps":"IOTAUSDT",         // 交易对标的
///     "ct":"PERPETUAL",        // 合约类型
///     "dt":4133404800000,      // 结算时间
///     "ot":1569398400000,      // 上架时间
///     "cs":"TRADING",          // 交易对状态
///     "bks":[
///         {
///             "bs":1,          // 层级
///             "bnf":0,         // 该层对应的名义价值下限
///             "bnc":5000,      // 该层对应的名义价值上限
///             "mmr":0.01,      // 该层对应的维持保证金率
///             "cf":0,          // 速算数
///             "mi":21,         // 该层杠杆下界
///             "ma":50          // 该层杠杆上界
///         },
///         {
///             "bs":2,
///             "bnf":5000,
///             "bnc":25000,
///             "mmr":0.025,
///             "cf":75,
///             "mi":11,
///             "ma":20
///         }
///     ]
/// }
/// ```
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ContractInfoPayload {
    /// 事件类型
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// 交易对标的
    #[serde(rename = "ps")]
    pub pair: String,

    /// 合约类型
    #[serde(rename = "ct")]
    pub contract_type: String,

    /// 结算时间
    #[serde(rename = "dt")]
    pub settlement_time: u64,

    /// 上架时间
    #[serde(rename = "ot")]
    pub open_time: u64,

    /// 交易对状态
    #[serde(rename = "cs")]
    pub contract_status: String,

    /// 持仓杠杆限制
    #[serde(rename = "bks")]
    pub bracket_limits: Option<Vec<BracketLimit>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BracketLimit {
    /// 层级
    #[serde(rename = "bs")]
    pub bracket_step: u32,

    /// 该层对应的名义价值下限
    #[serde(rename = "bnf", with = "string_to_decimal")]
    pub bracket_notional_floor: Decimal,

    /// 该层对应的名义价值上限
    #[serde(rename = "bnc", with = "string_to_decimal")]
    pub bracket_notional_cap: Decimal,

    /// 该层对应的维持保证金率
    #[serde(rename = "mmr", with = "string_to_decimal")]
    pub maintenance_margin_ratio: Decimal,

    /// 速算数
    #[serde(rename = "cf", with = "string_to_decimal")]
    pub cumulative_fee_factor: Decimal,

    /// 该层杠杆下界
    #[serde(rename = "mi")]
    pub leverage_min: u32,

    /// 该层杠杆上界
    #[serde(rename = "ma")]
    pub leverage_max: u32,
}

///
/// [多资产模式资产汇率指数](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/websocket-market-streams/Multi-Assets-Mode-Asset-Index)
/// ```json
/// [
///     {
///       "e":"assetIndexUpdate",
///       "E":1686749230000,
///       "s":"ADAUSD",         // asset index symbol
///       "i":"0.27462452",     // 指数价格
///       "b":"0.10000000",     // bid估值折扣
///      "a":"0.10000000",     // ask估值折扣
///       "B":"0.24716207",     // bid价格
///       "A":"0.30208698",     // ask价格
///       "q":"0.05000000",     // 自动兑换bid估值折扣
///       "g":"0.05000000",     // 自动兑换ask估值折扣
///       "Q":"0.26089330",     // 自动兑换bid价格
///       "G":"0.28835575"      // 自动兑换ask价格
///     },
///     {
///       "e":"assetIndexUpdate",
///       "E":1686749230000,
///       "s":"USDTUSD",
///       "i":"0.99987691",
///       "b":"0.00010000",
///       "a":"0.00010000",
///       "B":"0.99977692",
///       "A":"0.99997689",
///       "q":"0.00010000",
///       "g":"0.00010000",
///       "Q":"0.99977692",
///       "G":"0.99997689"
///     }
/// ]
/// ```
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AssetIndexPayload {
    /// 事件类型
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 资产指数symbol
    #[serde(rename = "s")]
    pub symbol: String,

    /// 指数价格（字符串数值）
    #[serde(rename = "i", with = "string_to_decimal")]
    pub index_price: Decimal,

    /// bid估值折扣（字符串数值）
    #[serde(rename = "b", with = "string_to_decimal")]
    pub bid_discount: Decimal,

    /// ask估值折扣（字符串数值）
    #[serde(rename = "a", with = "string_to_decimal")]
    pub ask_discount: Decimal,

    /// bid价格（字符串数值）
    #[serde(rename = "B", with = "string_to_decimal")]
    pub bid_price: Decimal,

    /// ask价格（字符串数值）
    #[serde(rename = "A", with = "string_to_decimal")]
    pub ask_price: Decimal,

    /// 自动兑换bid估值折扣（字符串数值）
    #[serde(rename = "q", with = "string_to_decimal")]
    pub auto_exchange_bid_discount: Decimal,

    /// 自动兑换ask估值折扣（字符串数值）
    #[serde(rename = "g", with = "string_to_decimal")]
    pub auto_exchange_ask_discount: Decimal,

    /// 自动兑换bid价格（字符串数值）
    #[serde(rename = "Q", with = "string_to_decimal")]
    pub auto_exchange_bid_price: Decimal,

    /// 自动兑换ask价格（字符串数值）
    #[serde(rename = "G", with = "string_to_decimal")]
    pub auto_exchange_ask_price: Decimal,
}

///
/// [当前交易时段](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/websocket-market-streams/Trading-Session-Stream)
///
/// ```json
///   {
///     "e": "EquityUpdate",  	// 事件类型, 也可以是CommodityUpdate
///     "E": 1765244143062,     // 事件时间
///     "t": 1765242000000,   	// 交易时段开始时间
///     "T": 1765270800000,		  // 交易时段结束时间
///     "S": "OVERNIGHT"        // 交易时段类型
///   }
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TradingSessionPayload {
    /// 事件类型
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 交易时段开始时间
    #[serde(rename = "t")]
    pub start_time: u64,

    /// 交易时段结束时间
    #[serde(rename = "T")]
    pub end_time: u64,

    /// 交易时段类型
    #[serde(rename = "S")]
    pub session_type: String,
}
