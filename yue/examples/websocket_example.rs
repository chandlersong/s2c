use li::tools::logs::setup_logger_all;
use log::LevelFilter;
use std::time::Duration;
use yue::websockets::WebSocketClient;

/// 这个示例演示如何使用 WebSocketClient 连接到币安的公开 WebSocket 流
///
/// 运行方式：
/// ```bash
/// RUST_LOG=info cargo run --example websocket_example
/// ```
#[tokio::main]
async fn main() {
    // 初始化日志 (需要设置 RUST_LOG 环境变量)
    let _ = setup_logger_all(Some(LevelFilter::Debug));

    // 示例1: 连接到币安的交易流
    let client =
        WebSocketClient::new_with_env_proxy("wss://stream.binance.com:9443/ws/btcusdt@trade").with_reconnect_interval(Duration::from_secs(5));

    println!("开始连接到币安 WebSocket...");
    println!("订阅 BTCUSDT 交易流");
    println!("按 Ctrl+C 停止");

    // 连接并持续运行，自动处理重连
    if let Err(e) = client.connect_and_run().await {
        eprintln!("WebSocket 错误: {}", e);
    }
}
