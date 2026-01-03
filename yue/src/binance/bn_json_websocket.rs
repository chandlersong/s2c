//! 主要处理binance的JSON WebSocket连接和消息处理。
//!
//! - [行情的推送借口](https://developers.binance.com/docs/zh-CN/binance-spot-api-docs/web-socket-streams#websocket-%E8%BF%9E%E6%8E%A5%E9%99%90%E5%88%B6)
//!

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(all(feature = "binance-testnet", not(test)))]
pub const SPOT_WEBSOCKET: &str = "wss://ws-api.testnet.binance.vision/ws-api/v3";

#[cfg(not(any(feature = "binance-testnet", test)))]
pub const SPOT_WEBSOCKET: &str = "wss://ws-api.binance.com:443/ws-api/v3";
#[cfg(all(feature = "binance-testnet", not(test)))]
pub const SPOT_STREAM_WEBSOCKET: &str = "wss://stream.testnet.binance.vision:9443";
#[cfg(not(any(feature = "binance-testnet", test)))]
pub const SPOT_STREAM_WEBSOCKET: &str = "wss://stream.binance.com:9443";
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

#[derive(Serialize)]
pub struct StreamCommandRequest {
    pub method: String,
    pub params: Vec<String>,
    pub id: u64,
}
