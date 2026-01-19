//! 主要处理binance的JSON WebSocket连接和消息处理。
//!
//! - [行情的推送借口](https://developers.binance.com/docs/zh-CN/binance-spot-api-docs/web-socket-streams#websocket-%E8%BF%9E%E6%8E%A5%E9%99%90%E5%88%B6)
//!

use crate::tools::SignatureContext;
use serde::Serialize;
use std::collections::HashMap;

#[cfg(all(feature = "binance-testnet", not(test)))]
pub const SPOT_WEBSOCKET: &str = "wss://ws-api.testnet.binance.vision/ws-api/v3";

#[cfg(not(any(feature = "binance-testnet", test)))]
pub const SPOT_WEBSOCKET: &str = "wss://ws-api.binance.com:443/ws-api/v3";
#[cfg(all(feature = "binance-testnet", not(test)))]
pub const SPOT_STREAM_WEBSOCKET: &str = "wss://stream.testnet.binance.vision:9443/ws";
#[cfg(not(any(feature = "binance-testnet", test)))]
pub const SPOT_STREAM_WEBSOCKET: &str = "wss://stream.binance.com:9443/ws";
pub const SPOT_DATA_STREAM_WEBSOCKET: &str = "wss://data-stream.binance.vision";
pub const WS_PING_COMMAND: &str = "ping";
pub const WS_TIME_COMMAND: &str = "time";
pub const WS_SUBSCRIBE_COMMAND: &str = "SUBSCRIBE";
pub const WS_SET_PROPERTY_COMMAND: &str = "SET_PROPERTY";
pub const WS_GET_PROPERTY_COMMAND: &str = "GET_PROPERTY";

#[derive(Serialize)]
pub struct CommandRequest {
    pub method: String,
    pub params: HashMap<String, String>,
    pub id: u64,
}

impl CommandRequest {
    pub fn new(method: &str, params: HashMap<String, String>, id: u64) -> Self {
        CommandRequest {
            method: method.to_string(),
            params,
            id,
        }
    }
}

impl SignatureContext for CommandRequest {
    fn context_for_signature(&self) -> String {
        let mut kvs: Vec<_> = self.params.iter().filter(|(k, _)| k.as_str() != "signature").collect();
        kvs.sort_by(|a, b| a.0.cmp(b.0));
        kvs.into_iter().map(|(k, v)| format!("{}={}", k, v)).collect::<Vec<_>>().join("&")
    }
}

#[derive(Serialize)]
pub struct StreamCommandRequest {
    pub method: String,
    pub params: Vec<String>,
    pub id: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_context_sorts_and_joins_params() {
        let mut params = HashMap::new();
        params.insert("c".to_string(), "3".to_string());
        params.insert("a".to_string(), "1".to_string());
        params.insert("b".to_string(), "2".to_string());

        let req = CommandRequest::new("test", params, 1);
        assert_eq!(req.context_for_signature(), "a=1&b=2&c=3");
    }

    #[test]
    fn signature_context_handles_empty_params() {
        let req = CommandRequest::new("test", HashMap::new(), 1);
        assert_eq!(req.context_for_signature(), "");
    }

    #[test]
    fn signature_context_skips_signature_key() {
        let mut params = HashMap::new();
        params.insert("signature".to_string(), "sig".to_string());
        params.insert("a".to_string(), "1".to_string());
        params.insert("b".to_string(), "2".to_string());

        let req = CommandRequest::new("test", params, 1);
        assert_eq!(req.context_for_signature(), "a=1&b=2");
    }
}
