use actix::Message;
/// 最简单的 WebSocketClient 使用示例
///
/// 这个示例展示如何：
/// 1. 从环境变量读取代理配置
/// 2. 创建 Actor 并订阅 WebSocket 事件
/// 3. 处理各种事件类型
use li::errors::LiError;
use li::tools::logs::setup_logger;
use li::websocket::connection::WebSocketConnection;
use li::websocket::models::WebSocketMessage;
use log::{LevelFilter, info};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::broadcast::Receiver;

#[derive(Clone, Message)]
#[rtype(result = "()")]
#[derive(Debug)]
struct TextMessage(String);

impl WebSocketMessage for TextMessage {
    fn from_text(s: &str) -> Result<Self, LiError> {
        Ok(TextMessage(s.to_string()))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    setup_logger(
        Some(LevelFilter::Warn),
        HashMap::from([
            ("websocket_simple_example".to_string(), LevelFilter::Trace),
            ("li".to_string(), LevelFilter::Trace),
        ]),
    )
    .expect("日志初始化失败");

    // 步骤 1: 创建 WebSocket 客户端
    // 自动从环境变量读取代理配置
    let url = "wss://stream.binance.com:9443/stream?streams=btcusdt@ticker/btcusdt@kline_5m".to_string();
    let reconnect_interval = Duration::from_secs(5);
    let proxy = Some("http://127.0.0.1:7891".to_string());
    let interface = WebSocketConnection::run(url, reconnect_interval, proxy, None).await;
    info!("✓ WebSocket 客户端已启动");

    let mut message_rx: Receiver<TextMessage> = interface.get_message_receiver().unwrap();

    tokio::spawn(async move {
        while let Ok(message) = message_rx.recv().await {
            info!("Received message: {:?}", message);
        }
    });

    let mut event_rx = interface.get_event_broadcast();
    tokio::spawn(async move {
        while let Ok(message) = event_rx.recv().await {
            info!("Received message: {:?}", message);
        }
    });

    info!("✓ 事件处理器已订阅");
    info!("等待 WebSocket 事件...\n");

    // 步骤 4: 运行 10 秒
    tokio::time::sleep(Duration::from_secs(60)).await;

    info!("\n========== 示例结束 ==========");
    Ok(())
}
