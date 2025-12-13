use li::tools::logs::setup_logger_all;
use log::LevelFilter;
use std::time::Duration;
use yue::websockets::WebSocketClient;

/// 这个示例演示如何使用 WebSocketClient 连接到币安的公开 WebSocket 流
/// 并在运行时发送消息
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
    let client = WebSocketClient::new_with_env_proxy("wss://stream.binance.com:9443/ws/btcusdt@trade")
        .with_proxy("http://127.0.0.1:7891")
        .with_reconnect_interval(Duration::from_secs(5));

    println!("开始连接到币安 WebSocket...");
    println!("订阅 BTCUSDT 交易流");
    println!("按 Ctrl+C 停止");

    // 连接并持续运行（在后台任务），返回 sender
    let sender = client.connect_and_run();

    // 等待连接建立
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 示例：发送订阅消息（如果需要的话）
    // let subscribe_msg = r#"{"method":"SUBSCRIBE","params":["btcusdt@depth"],"id":1}"#;
    // if let Err(e) = sender.send_text(subscribe_msg) {
    //     eprintln!("发送订阅消息失败: {}", e);
    // }

    // 示例：定期发送心跳 Ping
    let sender_clone = sender.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            if let Err(e) = sender_clone.send_ping(vec![]) {
                eprintln!("发送 Ping 失败: {}", e);
            }
        }
    });

    // 保持主线程运行
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}
