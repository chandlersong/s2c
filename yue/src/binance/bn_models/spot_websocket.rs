use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BinanceSpotWebsocket {
    RecentTrades(RecentTradesResponse),
}

impl BinanceSpotWebsocket {
    pub fn from_text(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}

/// 单条最近成交记录，对应 Binance WebSocket API `recentTrades` 返回的元素。
#[derive(Debug, Deserialize, Serialize)]
pub struct RecentTrade {
    /// 成交记录唯一标识。
    #[serde(rename = "id")]
    pub id: u64,
    /// 成交价格，字符串形式，避免浮点精度损失。
    #[serde(rename = "price")]
    pub price: String,
    /// 成交数量（base 资产数量），字符串形式。
    #[serde(rename = "qty")]
    pub qty: String,
    /// 按报价资产计价的成交量，字符串形式。
    #[serde(rename = "quoteQty")]
    pub quote_qty: String,
    /// 成交时间戳（毫秒）。
    #[serde(rename = "time")]
    pub time: u64,
    /// 是否为买方挂单成交方（taker 为卖单）。
    #[serde(rename = "isBuyerMaker")]
    pub is_buyer_maker: bool,
    /// 是否为最佳匹配。
    #[serde(rename = "isBestMatch")]
    pub is_best_match: bool,
}

/// WebSocket API `recentTrades` 的完整响应载体。
#[derive(Debug, Deserialize, Serialize)]
pub struct RecentTradesResponse {
    /// 请求 id，对应发送时的 id。
    pub id: u64,
    /// HTTP 风格的状态码，200 代表成功。
    pub status: u16,
    /// 最近成交列表。
    pub result: Vec<RecentTrade>,
}
