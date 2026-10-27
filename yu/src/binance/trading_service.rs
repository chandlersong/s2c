use crate::binance::bn_backend_service::get_spot_trading_table;
use crate::binance::models::SpotStreamTradeRecordPo;
use crate::duck_db_tables::DuckTableTableChannel;
use async_trait::async_trait;
use li::websocket::connection::{CommandMessage, MessageHandlerTrait, ToServerMessage, WebSocketConnection, WebSocketInterface};
use std::sync::Arc;
use yue::binance::bn_json_websocket::{SPOT_STREAM_WEBSOCKET, StreamCommandRequest, WS_SUBSCRIBE_COMMAND};
use yue::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamWrapper;
use yue::errors::YueError;

pub struct TradingSaver {
    _table: DuckTableTableChannel<SpotStreamTradeRecordPo>,
}

#[async_trait]
impl MessageHandlerTrait<BinanceSpotWebSocketStreamWrapper> for TradingSaver {
    async fn handle_message(&self, _message: &BinanceSpotWebSocketStreamWrapper) {
        // TODO: 以后改吧。说不定可以删掉。
        // trace!("Received WebSocket message: {:?}", message);
        // match &message.data {
        //     BinanceSpotWebSocketStreamResponse::Trade(trade) => {
        //         let po = SpotStreamTradeRecordPo::from(trade.clone());
        //         let insert_command = QueryCommand::Insert(InsertPayload::new_no_replay(po));
        //         if let Err(e) = self._table.send(insert_command).await {
        //             error!("Failed to send insert command to trade: {}", e);
        //         }
        //     }
        //     _ => {
        //         trace!("Received non-depth update message, ignoring");
        //     }
        // }
        todo!("TradingSaver handle_message is not implemented yet, please implement it to handle trade messages and insert them into the database.")
    }
}

pub struct TradingService {
    /// 所有symbol的订单簿快照
    web_socket_interface: Arc<WebSocketInterface<BinanceSpotWebSocketStreamWrapper>>,
}

impl TradingService {
    pub async fn spot(proxy: Option<String>) -> Self {
        let saver = Arc::new(TradingSaver {
            _table: get_spot_trading_table(),
        });
        let interface =
            WebSocketConnection::run::<BinanceSpotWebSocketStreamWrapper>(SPOT_STREAM_WEBSOCKET.to_string(), None, proxy, Some(saver)).await;
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
