/// OrderBookService 示例
///
/// 这个示例展示如何：
/// 1. 创建并启动 OrderBookService
/// 2. 连接到币安 WebSocket 获取深度更新数据
/// 3. 订阅 OrderBook 快照并打印
/// 4. 自动处理订单簿的初始化和增量更新
use async_trait::async_trait;
use li::tools::logs::setup_logger;
use li::websocket::connection::{MessageHandlerTrait, ToServerMessage, WebSocketConnection};
use log::{LevelFilter, info};
use serde_json::to_string;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use yue::binance::bn_json_websocket::{SPOT_STREAM_WEBSOCKET, StreamCommandRequest, WS_SUBSCRIBE_COMMAND};
use yue::binance::bn_models::spot_websocket_stream::{
    BinanceSpotWebSocketStreamResponse, BinanceSpotWebSocketStreamWrapper, DepthUpdateStreamPayload,
};
use yue::binance::order_book::initial_order_book;
use yue::http_client::init_http_client;

struct WebsocketSubscribe {
    tx: mpsc::UnboundedSender<DepthUpdateStreamPayload>,
}

impl WebsocketSubscribe {
    fn new(tx: mpsc::UnboundedSender<DepthUpdateStreamPayload>) -> WebsocketSubscribe {
        Self { tx }
    }
}
#[async_trait]
impl MessageHandlerTrait<BinanceSpotWebSocketStreamWrapper> for WebsocketSubscribe {
    async fn handle_message(&self, message: &BinanceSpotWebSocketStreamWrapper) {
        match &message.data {
            BinanceSpotWebSocketStreamResponse::DepthUpdate(d) => self.tx.send(d.clone()).unwrap(),
            _ => {}
        };
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    let mut special_log = HashMap::new();
    special_log.insert("yue".to_string(), LevelFilter::Info);
    special_log.insert("li".to_string(), LevelFilter::Info);
    special_log.insert("inital_order_book_example".to_string(), LevelFilter::Info);
    setup_logger(Some(LevelFilter::Warn), special_log)?;

    info!("========== OrderBookService 示例 ==========");
    info!("本示例展示如何使用 OrderBookService 维护实时订单簿");

    // 初始化 HTTP 客户端（用于获取初始深度数据）
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy.clone());
    info!("✓ HTTP 客户端已初始化");
    let (tx, rx) = mpsc::unbounded_channel();
    let reconnect_interval = Duration::from_secs(5);
    let handler = Arc::new(WebsocketSubscribe { tx });
    let interface = WebSocketConnection::run::<BinanceSpotWebSocketStreamWrapper>(
        SPOT_STREAM_WEBSOCKET.to_string(),
        reconnect_interval,
        Some(proxy.unwrap().to_string()),
        Some(handler),
    )
    .await;
    info!("✓ 订阅请求已发送");
    let subscribe_request = StreamCommandRequest {
        method: WS_SUBSCRIBE_COMMAND.to_string(),
        params: vec!["ethusdt@depth@100ms".to_string()],
        id: 1,
    };
    let command_test = to_string(&subscribe_request)?;
    interface.send_command(li::websocket::connection::CommandMessage::ToServer(ToServerMessage::text(command_test)));
    // 再运行 5 秒
    tokio::time::sleep(Duration::from_secs(5)).await;
    let order_book = initial_order_book(String::from("ETHUSDT"), rx).await;

    info!("初始化orderbook");
    info!(
        "OrderBookService 将自动:{},last update id:{}",
        order_book.symbol, order_book.local_update_id
    );
    info!("ask num:{}", order_book.asks().len());
    info!("bids num:{}", order_book.bids().len());

    Ok(())
}
