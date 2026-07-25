use bon::Builder;
use li::errors::LiError;
use li::websocket::models::WebSocketMessage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum OkxWebsocketResponse {
    SubscribeResponse(SubscribeResponsePayload),
    Kline(KlinePayload),
}

impl WebSocketMessage for OkxWebsocketResponse {
    fn from_text(text: &str) -> Result<Self, LiError> {
        serde_json::from_str(text).map_err(|e| LiError::from(e))
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Builder)]
pub struct ArgBody {
    #[serde(rename = "channel")]
    pub channel: String,

    #[serde(rename = "instId")]
    pub inst_id: String,
}

///
/// [参考](https://www.okx.com/docs-v5/zh/#order-book-trading-market-data-ws-candlesticks-channel)
///
#[derive(Debug, Deserialize, Serialize, Clone, Builder)]
pub struct SubscribeResponsePayload {
    /// 事件类型
    #[serde(rename = "id")]
    pub id: String,

    /// 事件时间
    #[serde(rename = "event")]
    pub event: Option<String>,

    #[serde(rename = "code")]
    pub code: Option<String>,

    #[serde(rename = "msg")]
    pub msg: Option<String>,

    #[serde(rename = "arg")]
    pub arg: Option<ArgBody>,

    #[serde(rename = "connId")]
    pub conn_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Builder)]
pub struct KlinePayload {
    #[serde(rename = "arg")]
    pub arg: ArgBody,

    #[serde(rename = "data")]
    pub data: Vec<Vec<String>>,
}
