use crate::binance::models::{DepthRecordPo, TradeRecordPo};
use crate::errors::YuError;
use yue::binance::bn_models::spot_websocket_stream::{BinanceSpotEvent, BinanceSpotWebSocketStream};
use std::str::FromStr;

#[derive(Debug)]
pub enum ParseResult {
    Trade(TradeRecordPo),
    Depth(DepthRecordPo),
}

pub struct SpotMessageParser;

impl SpotMessageParser {
    /// 解析文本消息并路由到 TradeRecordPo 或 DepthRecordPo
    pub fn parse_and_route(text: &str) -> Result<Option<ParseResult>, YuError> {
        let stream = BinanceSpotWebSocketStream::from_text(text)
            .map_err(|e| YuError::new(&format!("parse stream: {}", e)))?;
        match stream {
            BinanceSpotWebSocketStream::Event(event) => match event {
                BinanceSpotEvent::Trade(p) => {
                    let price = f64::from_str(&p.price).map_err(|e| YuError::new(&format!("parse price: {}", e)))?;
                    let qty = f64::from_str(&p.qty).map_err(|e| YuError::new(&format!("parse qty: {}", e)))?;
                    let po = TradeRecordPo {
                        event_time: p.event_time as i64,
                        symbol: p.symbol,
                        trade_id: p.trade_id as i64,
                        price,
                        qty,
                        buyer_order_id: Some(p.buyer_order_id as i64),
                        seller_order_id: Some(p.seller_order_id as i64),
                        trade_time: Some(p.trade_time as i64),
                        is_buyer_maker: Some(p.is_buyer_maker),
                        created_at: chrono::Utc::now().timestamp_millis(),
                    };
                    Ok(Some(ParseResult::Trade(po)))
                }
                BinanceSpotEvent::DepthUpdate(p) => {
                    // convert bids/asks to JSON strings
                    let bids = serde_json::to_string(&p.bids).map_err(|e| YuError::new(&format!("serialize bids: {}", e)))?;
                    let asks = serde_json::to_string(&p.asks).map_err(|e| YuError::new(&format!("serialize asks: {}", e)))?;
                    let po = DepthRecordPo {
                        event_time: p.event_time as i64,
                        symbol: p.symbol,
                        first_update_id: Some(p.first_update_id as i64),
                        final_update_id: p.final_update_id as i64,
                        prev_final_update_id: p.prev_final_update_id.map(|v| v as i64),
                        bids_json: Some(bids),
                        asks_json: Some(asks),
                        created_at: chrono::Utc::now().timestamp_millis(),
                    };
                    Ok(Some(ParseResult::Depth(po)))
                }
                _ => Ok(None),
            },
            // 非 Event 类型（例如 BookTicker 等），忽略
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_trade_event() {
        let text = r#"{"e":"trade","E":1620000000000,"s":"BTCUSDT","t":12345,"p":"34123.12","q":"0.001","b":111,"a":222,"T":1620000001000,"m":false}"#;
        let res = SpotMessageParser::parse_and_route(text).expect("parse result");
        match res {
            Some(ParseResult::Trade(po)) => {
                assert_eq!(po.symbol, "BTCUSDT");
                assert_eq!(po.trade_id, 12345);
                let price_diff = (po.price - 34123.12_f64).abs();
                assert!(price_diff < 1e-8);
                let qty_diff = (po.qty - 0.001_f64).abs();
                assert!(qty_diff < 1e-12);
            }
            other => panic!("unexpected parse result: {:?}", other),
        }
    }

    #[test]
    fn test_parse_depth_event() {
        let text = r#"{"e":"depthUpdate","E":1620000002000,"s":"ETHUSDT","U":1000,"u":1001,"pu":999,"b":[["100.1","0.5"]],"a":[["100.2","0.6"]]}"#;
        let res = SpotMessageParser::parse_and_route(text).expect("parse result");
        match res {
            Some(ParseResult::Depth(po)) => {
                assert_eq!(po.symbol, "ETHUSDT");
                assert_eq!(po.final_update_id, 1001);
                assert!(po.bids_json.as_ref().unwrap().contains("100.1"));
                assert!(po.asks_json.as_ref().unwrap().contains("100.2"));
            }
            other => panic!("unexpected parse result: {:?}", other),
        }
    }

    #[test]
    fn test_parse_invalid_json() {
        let text = "not a json";
        let res = SpotMessageParser::parse_and_route(text);
        assert!(res.is_err(), "expected parse error for invalid json");
    }
}

