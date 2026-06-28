use crate::binance::bn_backend_service::get_spot_trading_table;
use crate::binance::models::SpotStreamTradeRecordPo;
use crate::duck_db_tables::DuckTableTableChannel;
use async_trait::async_trait;
use li::websocket::connection::{CommandMessage, MessageHandlerTrait, ToServerMessage, WebSocketConnection, WebSocketInterface};
use log::{error, trace};
use std::sync::Arc;
use std::time::Duration;
use yue::binance::bn_json_websocket::{StreamCommandRequest, SPOT_STREAM_WEBSOCKET, WS_SUBSCRIBE_COMMAND};
use yue::binance::bn_models::spot_websocket_stream::{BinanceSpotWebSocketStreamResponse, BinanceSpotWebSocketStreamWrapper};
use yue::errors::YueError;
use yue::query_message::{InsertPayload, QueryCommand};

pub struct TradingSaver {
    table: DuckTableTableChannel<SpotStreamTradeRecordPo>,
}

#[async_trait]
impl MessageHandlerTrait<BinanceSpotWebSocketStreamWrapper> for TradingSaver {
    async fn handle_message(&self, message: &BinanceSpotWebSocketStreamWrapper) {
        trace!("Received WebSocket message: {:?}", message);
        match &message.data {
            BinanceSpotWebSocketStreamResponse::Trade(trade) => {
                let po = SpotStreamTradeRecordPo::from(trade.clone());
                let insert_command = QueryCommand::Insert(InsertPayload::new_no_replay(po));
                if let Err(e) = self.table.send(insert_command).await {
                    error!("Failed to send insert command to trade: {}", e);
                }
            }
            _ => {
                trace!("Received non-depth update message, ignoring");
            }
        }
    }
}

pub struct TradingService {
    /// 所有symbol的订单簿快照
    web_socket_interface: Arc<WebSocketInterface<BinanceSpotWebSocketStreamWrapper>>,
}

impl TradingService {
    pub async fn spot(proxy: Option<String>) -> Self {
        let saver = Arc::new(TradingSaver {
            table: get_spot_trading_table(),
        });
        let reconnect_interval = Duration::from_secs(5);
        let interface =
            WebSocketConnection::run::<BinanceSpotWebSocketStreamWrapper>(SPOT_STREAM_WEBSOCKET.to_string(), reconnect_interval, proxy, Some(saver))
                .await;
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
