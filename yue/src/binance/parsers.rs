use crate::binance::bn_models::spot_websocket::BinanceSpotWebSocketResponse;
use crate::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamResponse;
use crate::errors::YueError;
use crate::websocket::event_bus::WebSocketParser;
use log::trace;

/// Binance 现货公共行情解析器
pub struct BinanceSpotStreamParser;

impl WebSocketParser for BinanceSpotStreamParser {
    type Output = BinanceSpotWebSocketStreamResponse;

    fn parse_text(&self, text: &str) -> Result<Self::Output, YueError> {
        match BinanceSpotWebSocketStreamResponse::from_text(text) {
            Ok(response) => {
                trace!("✓ 成功解析币安现货行情: {:?}", response);
                Ok(response)
            }
            Err(e) => {
                let preview = if text.len() > 200 {
                    format!("{}...", &text[..200])
                } else {
                    text.to_string()
                };
                Err(YueError::ParseError(format!("解析币安现货行情失败: {}\n消息预览: {}", e, preview)))
            }
        }
    }

    fn parse_binary(&self, _data: &[u8]) -> Result<Self::Output, YueError> {
        Err(YueError::NotImplemented("binary parsing not implemented".to_string()))
    }
}

/// Binance 账户流解析器（余额/订单事件）
pub struct SpotAccountStreamParser;

impl WebSocketParser for SpotAccountStreamParser {
    type Output = BinanceSpotWebSocketResponse;

    fn parse_text(&self, text: &str) -> Result<Self::Output, YueError> {
        match BinanceSpotWebSocketResponse::from_text(text) {
            Ok(response) => {
                trace!("✓ 成功解析币安账户流: {:?}", response);
                Ok(response)
            }
            Err(e) => {
                let preview = if text.len() > 200 {
                    format!("{}...", &text[..200])
                } else {
                    text.to_string()
                };
                Err(YueError::ParseError(format!("解析币安账户流失败: {}\n消息预览: {}", e, preview)))
            }
        }
    }

    fn parse_binary(&self, _data: &[u8]) -> Result<Self::Output, YueError> {
        Err(YueError::NotImplemented("binary parsing not implemented".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::prelude::ToPrimitive;

    #[test]
    fn test_parse_trade_message() {
        let parser = BinanceSpotStreamParser;
        let trade_json = r#"{
            "e":"trade",
            "E":1234567890,
            "s":"BTCUSDT",
            "t":123456,
            "p":"40000.00",
            "q":"1.0",
            "T":1234567890,
            "m":false,
            "M":false
        }"#;

        let result = parser.parse_text(trade_json);
        assert!(result.is_ok(), "Failed to parse trade message: {:?}", result);

        match result.unwrap() {
            BinanceSpotWebSocketStreamResponse::Trade(trade) => {
                assert_eq!(trade.symbol, "BTCUSDT");
                assert_eq!(trade.trade_id, 123456);
                assert!((trade.price.to_f64().unwrap() - 40000.00).abs() < 0.01);
            }
            _ => panic!("Expected Trade variant"),
        }
    }

    #[test]
    fn test_parse_invalid_json() {
        let parser = BinanceSpotStreamParser;
        let invalid_json = r#"{"invalid": json}"#;

        let result = parser.parse_text(invalid_json);
        assert!(result.is_err(), "Should fail on invalid JSON");
    }

    #[test]
    fn test_parse_empty_string() {
        let parser = BinanceSpotStreamParser;
        let result = parser.parse_text("");
        assert!(result.is_err(), "Should fail on empty string");
    }

    #[test]
    fn test_parse_outbound_account_position() {
        let parser = SpotAccountStreamParser;
        let json = r#"{
            "subscriptionId": 123,
            "event": {
                "a":"outboundAccountPosition",
                "E":1690000000000,
                "u":1690000000000,
                "B":[
                    {
                        "a":"BTC",
                        "f":"1.5",
                        "l":"0.5"
                    },
                    {
                        "a":"USDT",
                        "f":"50000.0",
                        "l":"0.0"
                    }
                ]
            }
        }"#;

        let result = parser.parse_text(json);
        assert!(result.is_ok(), "Failed to parse account position: {:?}", result);
        match result.unwrap() {
            BinanceSpotWebSocketResponse::OutboundAccountPosition(payload) => {
                assert_eq!(payload.subscription_id, 123);
                assert_eq!(payload.event.event, "outboundAccountPosition");
                assert_eq!(payload.event.balances.len(), 2);
            }
            _ => panic!("Expected OutboundAccountPosition variant"),
        }
    }

    #[test]
    fn test_parse_subscribe_response() {
        let parser = SpotAccountStreamParser;
        let json = r#"{"id":1,"status":200,"result":{"subscriptionId":12345}}"#;
        let result = parser.parse_text(json);
        assert!(result.is_ok(), "Failed to parse subscribe response: {:?}", result);
        match result.unwrap() {
            BinanceSpotWebSocketResponse::SubscribeResponse(resp) => {
                assert_eq!(resp.status, Some(200));
                assert_eq!(resp.result.unwrap().subscription_id, 12345);
            }
            _ => panic!("Expected SubscribeResponse variant"),
        }
    }

    #[test]
    fn test_account_parser_invalid_json() {
        let parser = SpotAccountStreamParser;
        let invalid = "{";
        let result = parser.parse_text(invalid);
        assert!(result.is_err(), "Account parser should fail on invalid json");
    }
}
