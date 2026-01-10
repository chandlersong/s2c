use log::error;
use rmp_serde::encode::Error;
use rmp_serde::to_vec;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use yue::binance::bn_models::spot_websocket_stream::{PartialBookDepthStream, TradeStreamPayload};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotStreamPartialBookDepthPo {
    pub id: i64,
    pub final_update_id: u64,
    /// bids/asks 原始二进制（建议存 MsgPack/CBOR 压缩后的字节）。
    pub bids_bin: Option<Vec<u8>>,
    pub asks_bin: Option<Vec<u8>>,
    pub created_at: i64,
}

impl From<PartialBookDepthStream> for SpotStreamPartialBookDepthPo {
    fn from(value: PartialBookDepthStream) -> Self {
        let asks = match to_vec(&value.asks) {
            Ok(v) => Some(v),
            Err(e) => {
                error!("Error serializing partial book depth stream: {}", e);
                None
            }
        };
        let bids = match to_vec(&value.bids) {
            Ok(v) => Some(v),
            Err(e) => {
                error!("Error serializing partial book depth stream: {}", e);
                None
            }
        };
        SpotStreamPartialBookDepthPo {
            id: 0,
            final_update_id: value.last_update_id,
            bids_bin: bids,
            asks_bin: asks,
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

    #[test]
    fn test_price_qty_f64_precision_depth() {
        let orig = SpotStreamPartialBookDepthPo {
            id: 1,

            final_update_id: 1001,
            bids_bin: Some(vec![1u8, 2, 3]),
            asks_bin: Some(vec![4u8, 5, 6]),
            created_at: 1_641_000_101,
        };

        let j = serde_json::to_string(&orig).expect("serialize");
        let parsed: SpotStreamPartialBookDepthPo = serde_json::from_str(&j).expect("deserialize");

        assert_eq!(orig.final_update_id, parsed.final_update_id);
    }
}
