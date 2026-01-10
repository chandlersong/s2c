use crate::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamResponse;
use crate::errors::YueError;
use crate::websocket::event_bus::WebSocketParser;
use log::trace;

/// Binance Spot WebSocket 消息解析器
/// 解析 Binance 现货 WebSocket 流消息，输出 BinanceSpotWebSocketStreamResponse
pub struct BinanceSpotStreamParser;

impl WebSocketParser for BinanceSpotStreamParser {
    type Output = BinanceSpotWebSocketStreamResponse;

    fn parse_text(&self, text: &str) -> Result<Self::Output, YueError> {
        match BinanceSpotWebSocketStreamResponse::from_text(text) {
            Ok(response) => {
                trace!("✓ 成功解析币安现货消息: {:?}", response);
                Ok(response)
            }
            Err(e) => {
                let preview = if text.len() > 200 {
                    format!("{}...", &text[..200])
                } else {
                    text.to_string()
                };
                Err(YueError::ParseError(format!("解析币安现货消息失败: {}\n消息预览: {}", e, preview)))
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
            "m":false
        }"#;

        let result = parser.parse_text(trade_json);
        assert!(result.is_ok(), "Failed to parse trade message: {:?}", result);
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
}
