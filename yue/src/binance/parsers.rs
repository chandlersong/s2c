use crate::binance::bn_models::spot_websocket::BinanceSpotWebSocketResponse;
use crate::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamResponse;
use crate::errors::YueError;
use crate::websocket::event_bus::WebSocketParser;
use log::trace;
use std::collections::HashMap;
use std::sync::RwLock;

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
pub struct SpotAccountStreamParser {
    id_to_account: HashMap<u64, String>,
    subscription_to_account: RwLock<HashMap<u64, String>>, // 运行期维护订阅ID与账户名关系，读多写少用RwLock
}

impl SpotAccountStreamParser {
    pub fn new(id_to_account: HashMap<u64, String>) -> Self {
        Self {
            id_to_account,
            subscription_to_account: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_capacity(id_to_account: HashMap<u64, String>, capacity: usize) -> Self {
        Self {
            id_to_account,
            subscription_to_account: RwLock::new(HashMap::with_capacity(capacity)),
        }
    }

    /// 根据 subscription_id 获取账户名（使用读锁）
    fn get_account_name(&self, subscription_id: u64) -> Result<Option<String>, YueError> {
        let guard = self
            .subscription_to_account
            .read()
            .map_err(|_| YueError::ParseError("解析账户流失败: subscription 映射读锁获取失败".to_string()))?;
        Ok(guard.get(&subscription_id).cloned())
    }
}

impl Default for SpotAccountStreamParser {
    fn default() -> Self {
        Self::new(HashMap::new())
    }
}

impl WebSocketParser for SpotAccountStreamParser {
    type Output = BinanceSpotWebSocketResponse;

    fn parse_text(&self, text: &str) -> Result<Self::Output, YueError> {
        let parsed = match BinanceSpotWebSocketResponse::from_text(text) {
            Ok(response) => response,
            Err(e) => {
                let preview = if text.len() > 200 {
                    format!("{}...", &text[..200])
                } else {
                    text.to_string()
                };
                return Err(YueError::ParseError(format!("解析币安账户流失败: {}\n消息预览: {}", e, preview)));
            }
        };

        match parsed {
            BinanceSpotWebSocketResponse::SubscribeResponse(resp) => {
                if let (Some(id), Some(result)) = (resp.id, resp.result.clone()) {
                    if let Some(account_name) = self.id_to_account.get(&id) {
                        let mut guard = self
                            .subscription_to_account
                            .write()
                            .map_err(|_| YueError::ParseError("解析账户流失败: subscription 映射写锁获取失败".to_string()))?;
                        guard.insert(result.subscription_id, account_name.clone());
                    }
                }
                trace!("✓ 成功解析币安账户订阅响应: {:?}", resp);
                Ok(BinanceSpotWebSocketResponse::SubscribeResponse(resp))
            }
            BinanceSpotWebSocketResponse::OutboundAccountPosition(mut payload) => {
                let account_name = self.get_account_name(payload.subscription_id)?;

                if let Some(name) = account_name {
                    payload.account_name = Some(name);
                    trace!("✓ 成功解析币安账户余额变动: {:?}", payload);
                    Ok(BinanceSpotWebSocketResponse::OutboundAccountPosition(payload))
                } else {
                    Err(YueError::ParseError(format!(
                        "解析币安账户流失败: subscription_id={} 未找到账户映射",
                        payload.subscription_id
                    )))
                }
            }
            BinanceSpotWebSocketResponse::BalanceUpdate(mut payload) => {
                let account_name = self.get_account_name(payload.subscription_id)?;

                if let Some(name) = account_name {
                    payload.account_name = Some(name);
                    trace!("✓ 成功解析币安单资产余额更新: {:?}", payload);
                    Ok(BinanceSpotWebSocketResponse::BalanceUpdate(payload))
                } else {
                    Err(YueError::ParseError(format!(
                        "解析币安账户流失败: subscription_id={} 未找到账户映射",
                        payload.subscription_id
                    )))
                }
            }
            BinanceSpotWebSocketResponse::ExecutionReport(mut payload) => {
                let account_name = self.get_account_name(payload.subscription_id)?;

                if let Some(name) = account_name {
                    payload.account_name = Some(name);
                    trace!("✓ 成功解析币安订单执行报告: {:?}", payload);
                    Ok(BinanceSpotWebSocketResponse::ExecutionReport(payload))
                } else {
                    Err(YueError::ParseError(format!(
                        "解析币安账户流失败: subscription_id={} 未找到账户映射",
                        payload.subscription_id
                    )))
                }
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
        let mut id_map = HashMap::new();
        id_map.insert(42, "acc_test".to_string());
        let parser = SpotAccountStreamParser::new(id_map);

        let subscribe_json = r#"{"id":42,"status":200,"result":{"subscriptionId":123}}"#;
        parser.parse_text(subscribe_json).expect("subscribe should build mapping");
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
                assert_eq!(payload.account_name.as_deref(), Some("acc_test"));
            }
            _ => panic!("Expected OutboundAccountPosition variant"),
        }
    }

    #[test]
    fn test_parse_subscribe_response() {
        let parser = SpotAccountStreamParser::default();
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
        let parser = SpotAccountStreamParser::default();
        let invalid = "{";
        let result = parser.parse_text(invalid);
        assert!(result.is_err(), "Account parser should fail on invalid json");
    }

    #[test]
    fn test_account_event_with_account_mapping() {
        let mut id_map = HashMap::new();
        id_map.insert(1, "acc_a".to_string());
        let parser = SpotAccountStreamParser::new(id_map);

        let subscribe_json = r#"{"id":1,"status":200,"result":{"subscriptionId":999}}"#;
        parser.parse_text(subscribe_json).expect("subscribe response should parse");

        let account_event = r#"{
            "subscriptionId": 999,
            "event": {
                "a":"outboundAccountPosition",
                "E":1690000000000,
                "u":1690000000000,
                "B":[{"a":"BTC","f":"1.5","l":"0.5"}]
            }
        }"#;

        let parsed = parser.parse_text(account_event).expect("account event should parse");
        match parsed {
            BinanceSpotWebSocketResponse::OutboundAccountPosition(p) => {
                assert_eq!(p.account_name.as_deref(), Some("acc_a"));
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn test_account_event_missing_subscription_mapping() {
        let parser = SpotAccountStreamParser::default();
        let account_event = r#"{
            "subscriptionId": 321,
            "event": {
                "a":"outboundAccountPosition",
                "E":1690000000000,
                "u":1690000000000,
                "B":[{"a":"BTC","f":"1.5","l":"0.5"}]
            }
        }"#;

        let parsed = parser.parse_text(account_event);
        assert!(parsed.is_err());
    }
}
