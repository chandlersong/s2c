use serde::{Deserialize, Serialize};
use yue::binance::bn_models::spot_websocket_stream::TradeStreamPayload;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotStreamTradeRecordPo {
    pub id: i64,
    pub event_time: i64,
    pub symbol: String,
    pub trade_id: i64,
    pub price: f64,
    pub qty: f64,
    pub trade_time: Option<i64>,
    pub is_buyer_maker: Option<bool>,
    pub created_at: i64,
}

impl From<TradeStreamPayload> for SpotStreamTradeRecordPo {
    fn from(payload: TradeStreamPayload) -> Self {
        SpotStreamTradeRecordPo {
            id: 0,
            event_time: payload.event_time as i64,
            symbol: payload.symbol,
            trade_id: payload.trade_id as i64,
            price: payload.price,
            qty: payload.qty,
            trade_time: Some(payload.trade_time as i64),
            is_buyer_maker: Some(payload.is_buyer_maker),
            created_at: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_price_qty_f64_precision_trade() {
        let orig = SpotStreamTradeRecordPo {
            id: 1,
            event_time: 1_641_000_000,
            symbol: "BTCUSDT".to_string(),
            trade_id: 12345,
            price: 34123.12345678_f64,
            qty: 0.00012345_f64,
            trade_time: Some(1_641_000_001),
            is_buyer_maker: Some(false),
            created_at: 1_641_000_002,
        };

        let j = serde_json::to_string(&orig).expect("serialize");
        let parsed: SpotStreamTradeRecordPo = serde_json::from_str(&j).expect("deserialize");

        let price_diff = (orig.price - parsed.price).abs();
        let qty_diff = (orig.qty - parsed.qty).abs();

        assert!(price_diff < 1e-8, "price diff too large: {}", price_diff);
        assert!(qty_diff < 1e-12, "qty diff too large: {}", qty_diff);
    }
}
