use li::websocket::connection::{CommandMessage, ShareMessageHandler, ToServerMessage, WebSocketConnection, WebSocketInterface};
use std::sync::Arc;
use yue::binance::bn_json_websocket::{SPOT_STREAM_WEBSOCKET, StreamCommandRequest, WS_SUBSCRIBE_COMMAND};
use yue::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamWrapper;
use yue::errors::YueError;

pub struct TradingService {
    /// 所有symbol的订单簿快照
    web_socket_interface: Arc<WebSocketInterface<BinanceSpotWebSocketStreamWrapper>>,
}

impl TradingService {
    pub async fn spot(proxy: Option<String>, handler: ShareMessageHandler<BinanceSpotWebSocketStreamWrapper>) -> Self {
        let interface =
            WebSocketConnection::run::<BinanceSpotWebSocketStreamWrapper>(SPOT_STREAM_WEBSOCKET.to_string(), None, proxy, Some(handler)).await;
        Self {
            web_socket_interface: interface,
        }
    }

    pub async fn subscribe_trade(&self, symbol: String) -> Result<(), YueError> {
        let subscribe_symbol = format!("{}@trade", symbol.to_lowercase());
        let subscribe_request = StreamCommandRequest {
            method: WS_SUBSCRIBE_COMMAND.to_string(),
            params: vec![subscribe_symbol],
            id: 1,
        };
        let command_test = serde_json::to_string(&subscribe_request)?;
        self.web_socket_interface
            .send_command(CommandMessage::ToServer(ToServerMessage::text(command_test)));
        Ok(())
    }
}
