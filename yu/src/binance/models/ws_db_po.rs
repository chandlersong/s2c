use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecordPo {
    pub event_time: i64,
    pub symbol: String,
    pub trade_id: i64,
    pub price: f64,
    pub qty: f64,
    pub buyer_order_id: Option<i64>,
    pub seller_order_id: Option<i64>,
    pub trade_time: Option<i64>,
    pub is_buyer_maker: Option<bool>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepthRecordPo {
    pub event_time: i64,
    pub symbol: String,
    pub first_update_id: Option<i64>,
    pub final_update_id: i64,
    pub prev_final_update_id: Option<i64>,
    pub bids_json: Option<String>,
    pub asks_json: Option<String>,
    pub created_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_price_qty_f64_precision_trade() {
        let orig = TradeRecordPo {
            event_time: 1_641_000_000,
            symbol: "BTCUSDT".to_string(),
            trade_id: 12345,
            price: 34123.12345678_f64,
            qty: 0.00012345_f64,
            buyer_order_id: Some(111),
            seller_order_id: Some(222),
            trade_time: Some(1_641_000_001),
            is_buyer_maker: Some(false),
            created_at: 1_641_000_002,
        };

        let j = serde_json::to_string(&orig).expect("serialize");
        let parsed: TradeRecordPo = serde_json::from_str(&j).expect("deserialize");

        let price_diff = (orig.price - parsed.price).abs();
        let qty_diff = (orig.qty - parsed.qty).abs();

        assert!(price_diff < 1e-8, "price diff too large: {}", price_diff);
        assert!(qty_diff < 1e-12, "qty diff too large: {}", qty_diff);
    }

    #[test]
    fn test_price_qty_f64_precision_depth() {
        let orig = DepthRecordPo {
            event_time: 1_641_000_100,
            symbol: "ETHUSDT".to_string(),
            first_update_id: Some(1000),
            final_update_id: 1001,
            prev_final_update_id: Some(999),
            bids_json: Some("[]".to_string()),
            asks_json: Some("[]".to_string()),
            created_at: 1_641_000_101,
        };

        // DepthRecordPo currently has no price/qty fields, but ensure serde roundtrip works
        let j = serde_json::to_string(&orig).expect("serialize");
        let parsed: DepthRecordPo = serde_json::from_str(&j).expect("deserialize");

        assert_eq!(orig.event_time, parsed.event_time);
        assert_eq!(orig.symbol, parsed.symbol);
        assert_eq!(orig.final_update_id, parsed.final_update_id);
    }
}
