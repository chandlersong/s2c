use crate::utils::get_snowflake_generator;
use rust_decimal::prelude::ToPrimitive;
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
            id: get_snowflake_generator().lock().unwrap().real_time_generate(),
            event_time: payload.event_time as i64,
            symbol: payload.symbol,
            trade_id: payload.trade_id as i64,
            price: payload.price.to_f64().unwrap(),
            qty: payload.qty.to_f64().unwrap(),
            trade_time: Some(payload.trade_time as i64),
            is_buyer_maker: Some(payload.is_buyer_maker),
            created_at: 0,
        }
    }
}

pub struct SpotBalancePo {
    pub account_id: String,
    pub asset: String,
    pub free: f64,
    pub locked: f64,
    pub event_time: i64,
    pub source_exchange: String,
    pub raw_json: String,
}

pub struct SpotOrderPo {
    pub account_id: String,
    pub symbol: String,
    pub order_id: String,
    pub client_order_id: String,
    pub status: String,
    pub side: String,
    pub order_type: String,
    pub price: f64,
    pub qty: f64,
    pub exec_qty: f64,
    pub last_exec_price: f64,
    pub event_time: i64,
    pub source_exchange: String,
    pub raw_json: String,
}
