use crate::binance::bn_models::common::map_depth_levels;
use crate::models::Decimal;
use crate::tools::string_to_decimal;
use actix::Message as ActixMessage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
/// 现货公共流的顶层反序列化入口，兼容单条事件和数组推送。
/// 支持的流类型：逐笔交易、归集交易、K线、有限档深度、按Symbol的最优挂单
///
/// 注意：枚举变体的顺序很重要，应该按照消息特征的唯一性从高到低排列，
/// 以确保 #[serde(untagged)] 能正确识别每种类型。
pub enum BinanceSpotWebSocketStreamResponse {
    /// K线流 - 最具特征性（包含 `k` 嵌套对象）
    Kline(KlineStreamPayload),
    /// 有限档深度快照 - 包含唯一的 `lastUpdateId` 字段
    PartialDepth(PartialBookDepthStream),
    /// 归集交易流 - 包含唯一的 `f` 和 `l` 字段
    AggTrade(AggTradeStreamPayload),
    /// 逐笔交易流 - 包含 `t` (trade_id) 字段
    Trade(TradeStreamPayload),
    /// 按Symbol的最优挂单 - 包含 `B` 和 `A` (大写) 字段
    BookTicker(BookTickerStreamPayload),
    /// book depth增量
    DepthUpdate(DepthUpdateStreamPayload),
}

impl ActixMessage for BinanceSpotWebSocketStreamResponse {
    type Result = ();
}

impl BinanceSpotWebSocketStreamResponse {
    pub fn from_text(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
/// 逐笔成交事件字段，对应 `trade`。
pub struct TradeStreamPayload {
    #[serde(rename = "e")]
    /// 事件时间 (ms)。
    pub event: String,
    #[serde(rename = "E")]
    /// 事件时间 (ms)。
    pub event_time: u64,
    #[serde(rename = "s")]
    /// 交易对符号。
    pub symbol: String,
    #[serde(rename = "t")]
    /// 成交 ID。
    pub trade_id: u64,
    #[serde(rename = "p")]
    /// 成交价格。
    #[serde(with = "string_to_decimal")]
    pub price: Decimal,
    #[serde(rename = "q")]
    #[serde(with = "string_to_decimal")]
    /// 成交数量。
    pub qty: Decimal,

    #[serde(rename = "T")]
    /// 成交时间戳 (ms)。
    pub trade_time: u64,
    #[serde(rename = "m")]
    /// 是否买方为挂单方（true 表示卖方主动成交）。
    pub is_buyer_maker: bool,

    #[serde(rename = "M")]
    /// 是否买方为挂单方（true 表示卖方主动成交）。
    pub ignore: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
/// 有限档深度快照（5/10/20 档）。
pub struct PartialBookDepthStream {
    #[serde(rename = "lastUpdateId")]
    /// 快照对应的 lastUpdateId。
    pub last_update_id: u64,
    #[serde(rename = "bids", deserialize_with = "map_depth_levels")]
    /// 买盘 [price, qty] 列表。
    pub bids: Vec<(f64, f64)>,
    #[serde(rename = "asks", deserialize_with = "map_depth_levels")]
    /// 卖盘 [price, qty] 列表。
    pub asks: Vec<(f64, f64)>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
/// 增量深度事件，对应 `depthUpdate`。
pub struct DepthUpdateStreamPayload {
    #[serde(rename = "e")]
    /// 事件时间 (ms)。
    pub event: String,
    #[serde(rename = "E")]
    /// 事件时间 (ms)。
    pub event_time: u64,
    #[serde(rename = "s")]
    /// 交易对符号。
    pub symbol: String,
    #[serde(rename = "U")]
    /// 首个更新 ID。
    pub first_update_id: u64,
    #[serde(rename = "u")]
    /// 最终更新 ID。
    pub final_update_id: u64,

    #[serde(rename = "b", deserialize_with = "map_depth_levels")]
    /// 买盘增量 [price, qty]。
    pub bids: Vec<(f64, f64)>,
    #[serde(rename = "a", deserialize_with = "map_depth_levels")]
    /// 卖盘增量 [price, qty]。
    pub asks: Vec<(f64, f64)>,
}

// 本地反序列化工具：将 [["price","qty"], ...] 转换为 Vec<(f64, f64)>
#[derive(Debug, Deserialize, Serialize, Clone)]
/// K 线事件载体，对应 `kline`。
pub struct KlineStreamPayload {
    #[serde(rename = "e")]
    /// 事件时间 (ms)。
    pub event: String,
    #[serde(rename = "E")]
    /// 事件时间 (ms)。
    pub event_time: u64,
    #[serde(rename = "s")]
    /// 交易对符号。
    pub symbol: String,
    #[serde(rename = "k")]
    /// K 线细节。
    pub kline: KlineData,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
/// K 线细节字段，含开收高低与量。
pub struct KlineData {
    #[serde(rename = "t")]
    /// K 线开始时间 (ms)。
    pub start_time: u64,
    #[serde(rename = "T")]
    /// K 线结束时间 (ms)。
    pub close_time: u64,
    #[serde(rename = "s")]
    /// 交易对符号。
    pub symbol: String,
    #[serde(rename = "i")]
    /// K 线周期（如 "1m"）。
    pub interval: String,
    #[serde(rename = "f")]
    /// 第一笔成交 ID。
    pub first_trade_id: u64,
    #[serde(rename = "L")]
    /// 最后一笔成交 ID。
    pub last_trade_id: u64,
    #[serde(rename = "o")]
    #[serde(with = "string_to_decimal")]
    /// 开盘价。
    pub open: Decimal,

    #[serde(rename = "c")]
    #[serde(with = "string_to_decimal")]
    /// 收盘价。
    pub close: Decimal,

    #[serde(rename = "h")]
    #[serde(with = "string_to_decimal")]
    /// 最高价。
    pub high: Decimal,
    #[serde(rename = "l")]
    #[serde(with = "string_to_decimal")]
    /// 最低价。
    pub low: Decimal,
    #[serde(rename = "v")]
    #[serde(with = "string_to_decimal")]
    /// 成交量（基准资产）。
    pub volume: Decimal,
    #[serde(rename = "n")]
    /// 成交笔数。
    pub trade_count: u64,
    #[serde(rename = "x")]
    /// 本根 K 线是否已闭合。
    pub is_closed: bool,
    #[serde(rename = "q")]
    #[serde(with = "string_to_decimal")]
    /// 成交量（按报价资产）。
    pub quote_volume: Decimal,
    #[serde(rename = "V")]
    #[serde(with = "string_to_decimal")]
    /// 主动买入成交量（基准资产）。
    pub taker_buy_base_volume: Decimal,
    #[serde(rename = "Q")]
    #[serde(with = "string_to_decimal")]
    /// 主动买入成交量（报价资产）。
    pub taker_buy_quote_volume: Decimal,
    #[serde(rename = "B")]
    /// 主动买入成交量（报价资产）。
    pub ignore: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
/// 归集成交事件，对应 `aggTrade`。
pub struct AggTradeStreamPayload {
    #[serde(rename = "e")]
    /// 事件类型（"aggTrade"）。
    pub event: String,
    #[serde(rename = "E")]
    /// 事件时间 (ms)。
    pub event_time: u64,
    #[serde(rename = "s")]
    /// 交易对符号。
    pub symbol: String,
    #[serde(rename = "a")]
    /// 归集成交 ID。
    pub agg_id: u64,
    #[serde(rename = "p")]
    #[serde(with = "string_to_decimal")]
    /// 成交价格。
    pub price: Decimal,
    #[serde(rename = "q")]
    #[serde(with = "string_to_decimal")]
    /// 成交数量。
    pub qty: Decimal,
    #[serde(rename = "f")]
    /// 首笔成交 ID。
    pub first_trade_id: u64,
    #[serde(rename = "l")]
    /// 尾笔成交 ID。
    pub last_trade_id: u64,
    #[serde(rename = "T")]
    /// 成交时间戳 (ms)。
    pub trade_time: u64,
    #[serde(rename = "m")]
    /// 是否买方为挂单方。
    pub is_buyer_maker: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
/// 最优挂单事件或数组元素，对应 `bookTicker`。
pub struct BookTickerStreamPayload {
    #[serde(rename = "u")]
    /// 更新 ID。
    pub update_id: u64,
    #[serde(rename = "s")]
    /// 交易对符号。
    pub symbol: String,
    #[serde(rename = "b")]
    #[serde(with = "string_to_decimal")]
    /// 最优买价。
    pub best_bid_price: Decimal,
    #[serde(rename = "B")]
    #[serde(with = "string_to_decimal")]
    /// 最优买量。
    pub best_bid_qty: Decimal,
    #[serde(rename = "a")]
    #[serde(with = "string_to_decimal")]
    /// 最优卖价。
    pub best_ask_price: Decimal,
    #[serde(rename = "A")]
    #[serde(with = "string_to_decimal")]
    /// 最优卖量。
    pub best_ask_qty: Decimal,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
/// 24h 精简 Ticker，对应 `24hrMiniTicker` 或全市场数组。
pub struct MiniTickerStreamPayload {
    #[serde(rename = "E")]
    /// 事件时间 (ms)。
    pub event_time: u64,
    #[serde(rename = "s")]
    /// 交易对符号。
    pub symbol: String,
    #[serde(rename = "c")]
    /// 最新价。
    pub close: String,
    #[serde(rename = "o")]
    /// 开盘价。
    pub open: String,
    #[serde(rename = "h")]
    /// 最高价。
    pub high: String,
    #[serde(rename = "l")]
    /// 最低价。
    pub low: String,
    #[serde(rename = "v")]
    /// 24h 成交量（基准资产）。
    pub volume: String,
    #[serde(rename = "q")]
    /// 24h 成交量（报价资产）。
    pub quote_volume: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
/// 24h 完整 Ticker，对应 `24hrTicker`。
pub struct TickerStreamPayload {
    #[serde(rename = "E")]
    /// 事件时间 (ms)。
    pub event_time: u64,
    #[serde(rename = "s")]
    /// 交易对符号。
    pub symbol: String,
    #[serde(rename = "p")]
    /// 价格变动额。
    pub price_change: String,
    #[serde(rename = "P")]
    /// 价格变动百分比。
    pub price_change_percent: String,
    #[serde(rename = "w")]
    /// 加权平均价。
    pub weighted_avg_price: String,
    #[serde(rename = "x")]
    /// 前收盘价。
    pub prev_close: String,
    #[serde(rename = "c")]
    /// 最新价。
    pub last_price: String,
    #[serde(rename = "Q")]
    /// 最新成交量。
    pub last_qty: String,
    #[serde(rename = "b")]
    /// 当前最优买价。
    pub bid_price: String,
    #[serde(rename = "B")]
    /// 当前最优买量。
    pub bid_qty: String,
    #[serde(rename = "a")]
    /// 当前最优卖价。
    pub ask_price: String,
    #[serde(rename = "A")]
    /// 当前最优卖量。
    pub ask_qty: String,
    #[serde(rename = "o")]
    /// 开盘价。
    pub open: String,
    #[serde(rename = "h")]
    /// 最高价。
    pub high: String,
    #[serde(rename = "l")]
    /// 最低价。
    pub low: String,
    #[serde(rename = "v")]
    /// 24h 成交量（基准资产）。
    pub volume: String,
    #[serde(rename = "q")]
    /// 24h 成交量（报价资产）。
    pub quote_volume: String,
    #[serde(rename = "O")]
    /// 统计起始时间 (ms)。
    pub open_time: u64,
    #[serde(rename = "C")]
    /// 统计结束时间 (ms)。
    pub close_time: u64,
    #[serde(rename = "F")]
    /// 首笔成交 ID。
    pub first_trade_id: u64,
    #[serde(rename = "L")]
    /// 尾笔成交 ID。
    pub last_trade_id: u64,
    #[serde(rename = "n")]
    /// 成交笔数。
    pub trade_count: u64,
}
