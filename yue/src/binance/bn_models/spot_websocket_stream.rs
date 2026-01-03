use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
/// 现货公共流的顶层反序列化入口，兼容事件单条和数组推送。
pub enum BinanceSpotWebSocketStream {
    /// ���带事件类型字段 `e` 的常规事件流。
    Event(BinanceSpotEvent),
    /// 单个最优挂单推送（无事件类型 tag，只有价格量字段）。
    BookTicker(BookTickerStreamPayload),
    /// 有限档深度快照（partial book depth）。
    PartialDepth(PartialBookDepthStream),
    /// 全市场精简 mini ticker 数组推送。
    MiniTickerArray(Vec<MiniTickerStreamPayload>),
    /// 全市场最优挂单数组推送。
    BookTickerArray(Vec<BookTickerStreamPayload>),
}

impl BinanceSpotWebSocketStream {
    pub fn from_text(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "e")]
/// 带事件类型 `e` 的现货公共流事件���举。
pub enum BinanceSpotEvent {
    #[serde(rename = "trade")]
    /// 逐笔成交（标的符号级）。
    Trade(TradeStreamPayload),
    #[serde(rename = "depthUpdate")]
    /// 增量深度（diff. depth）。
    DepthUpdate(DiffDepthStreamPayload),
    #[serde(rename = "kline")]
    /// K 线闭合/变更事件。
    Kline(KlineStreamPayload),
    #[serde(rename = "aggTrade")]
    /// 归集成交（多笔合并）。
    AggTrade(AggTradeStreamPayload),
    #[serde(rename = "24hrMiniTicker")]
    /// 24h 精简 Ticker。
    MiniTicker(MiniTickerStreamPayload),
    #[serde(rename = "24hrTicker")]
    /// 24h 完整 Ticker。
    Ticker(TickerStreamPayload),
}

#[derive(Debug, Deserialize, Serialize)]
/// 逐笔成交事件字段，对应 `trade`。
pub struct TradeStreamPayload {
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
    pub price: String,
    #[serde(rename = "q")]
    /// 成交数量。
    pub qty: String,
    #[serde(rename = "b")]
    /// 买方订单 ID。
    pub buyer_order_id: u64,
    #[serde(rename = "a")]
    /// 卖方订单 ID。
    pub seller_order_id: u64,
    #[serde(rename = "T")]
    /// 成交时间戳 (ms)。
    pub trade_time: u64,
    #[serde(rename = "m")]
    /// 是否买方为挂单方（true 表示卖方主动成交）。
    pub is_buyer_maker: bool,
}

#[derive(Debug, Deserialize, Serialize)]
/// 有限档深度快照（5/10/20 档）。
pub struct PartialBookDepthStream {
    #[serde(rename = "lastUpdateId")]
    /// 快照对应的 lastUpdateId。
    pub last_update_id: u64,
    #[serde(rename = "bids")]
    /// 买盘 [price, qty] 列表。
    pub bids: Vec<(String, String)>,
    #[serde(rename = "asks")]
    /// 卖盘 [price, qty] 列表。
    pub asks: Vec<(String, String)>,
}

#[derive(Debug, Deserialize, Serialize)]
/// 增量深度事件，对应 `depthUpdate`。
pub struct DiffDepthStreamPayload {
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
    /// 最��更新 ID。
    pub final_update_id: u64,
    #[serde(rename = "pu")]
    /// 上一条深度事件的最终更新 ID（可能缺失）。
    pub prev_final_update_id: Option<u64>,
    #[serde(rename = "b")]
    /// 买盘增量 [price, qty]。
    pub bids: Vec<(String, String)>,
    #[serde(rename = "a")]
    /// 卖盘增量 [price, qty]。
    pub asks: Vec<(String, String)>,
}

#[derive(Debug, Deserialize, Serialize)]
/// K 线事件载体，对应 `kline`。
pub struct KlineStreamPayload {
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

#[derive(Debug, Deserialize, Serialize)]
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
    /// 开盘价。
    pub open: String,
    #[serde(rename = "c")]
    /// 收盘价。
    pub close: String,
    #[serde(rename = "h")]
    /// 最高价。
    pub high: String,
    #[serde(rename = "l")]
    /// 最低价。
    pub low: String,
    #[serde(rename = "v")]
    /// 成交量（基准资产）。
    pub volume: String,
    #[serde(rename = "n")]
    /// 成交笔数。
    pub trade_count: u64,
    #[serde(rename = "x")]
    /// 本根 K 线是否已闭合。
    pub is_closed: bool,
    #[serde(rename = "q")]
    /// 成交量（按报价资产）。
    pub quote_volume: String,
    #[serde(rename = "V")]
    /// 主动买入成交量（基准资产）。
    pub taker_buy_base_volume: String,
    #[serde(rename = "Q")]
    /// 主动买入成交量（报价资产）。
    pub taker_buy_quote_volume: String,
}

#[derive(Debug, Deserialize, Serialize)]
/// 归集成交事件，对应 `aggTrade`。
pub struct AggTradeStreamPayload {
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
    /// 成交价格。
    pub price: String,
    #[serde(rename = "q")]
    /// 成交数量。
    pub qty: String,
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

#[derive(Debug, Deserialize, Serialize)]
/// 最优挂单事件或数组元素，对应 `bookTicker`。
pub struct BookTickerStreamPayload {
    #[serde(rename = "u")]
    /// 更新 ID。
    pub update_id: u64,
    #[serde(rename = "s")]
    /// 交易对符号。
    pub symbol: String,
    #[serde(rename = "b")]
    /// 最优买价。
    pub best_bid_price: String,
    #[serde(rename = "B")]
    /// 最优买量。
    pub best_bid_qty: String,
    #[serde(rename = "a")]
    /// 最优卖价。
    pub best_ask_price: String,
    #[serde(rename = "A")]
    /// 最优卖量。
    pub best_ask_qty: String,
}

#[derive(Debug, Deserialize, Serialize)]
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

#[derive(Debug, Deserialize, Serialize)]
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
