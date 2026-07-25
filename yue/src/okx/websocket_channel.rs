use crate::okx::models::websocket::ArgBody;
use bon::Builder;
use serde::Serialize;

pub const OXK_PUBLIC_WEBSOCKET: &str = "wss://ws.okx.com:8443/ws/v5/public";
pub const OXK_PRIVATE_WEBSOCKET: &str = "wss://ws.okx.com:8443/ws/v5/private";

pub const OXK_BUSINESS_WEBSOCKET: &str = "wss://ws.okx.com:8443/ws/v5/business";

#[derive(Serialize, Builder)]
pub struct CommandRequest {
    pub id: String,

    pub op: String,

    pub args: Vec<ArgBody>,
}
