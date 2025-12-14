use actix::{Actor, Context, Handler};
use li::tools::logs::setup_logger_all;
use log::{LevelFilter, info};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use yue::websocket::client::{SendTextMessage, SubscribeToEvents, WebSocketClient, WebSocketEvent};

/// 事件处理器 Actor，带连接状态追踪
struct EventHandler {}

impl Actor for EventHandler {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("🚀 EventHandler 已启动");
    }
}

/// WebSocketEvent 消息处理
impl Handler<WebSocketEvent> for EventHandler {
    type Result = ();

    fn handle(&mut self, event: WebSocketEvent, _ctx: &mut Context<Self>) -> Self::Result {
        match event {
            WebSocketEvent::Connected => {
                println!("✓ [事件] WebSocket 已连接");
                info!("✓ WebSocket 连接成功");
            }
            WebSocketEvent::Disconnected => {
                println!("✗ [事件] WebSocket 已断开");
                info!("✗ WebSocket 连接断开");
            }
            WebSocketEvent::TextMessage(text) => {
                // 只打印消息的前 200 个字符，避免输出过长
                let preview = if text.len() > 200 {
                    format!("{}...", &text[..200])
                } else {
                    text.clone()
                };
                println!("📨 [文本消息]\n{}\n", preview);
                info!("收到文本消息: {} 字符", text.len());
            }
            WebSocketEvent::BinaryMessage(data) => {
                println!("📦 [二进制消息] {} 字节", data.len());
                info!("收到二进制消息: {} 字节", data.len());
            }
            WebSocketEvent::Reconnecting => {
                println!("🔄 [事件] 正在重新连接...");
                info!("🔄 正在重新连接...");
            }
            WebSocketEvent::Error(err) => {
                println!("❌ [错误] {}", err);
                info!("❌ WebSocket 错误: {}", err);
            }
        }
    }
}

/// 这个示例演示如何使用 WebSocketClient Actor 连接到币安的公开 WebSocket 流
/// 并通过 Actor 的 Recipient 机制订阅消息
///
/// 运行方式：
/// ```bash
/// RUST_LOG=info cargo run --example websocket_example
/// ```
#[actix::main]
async fn main() {
    // 初始化日志
    let _ = setup_logger_all(Some(LevelFilter::Info));

    println!("╔══════════════════════════════════════════╗");
    println!("║   WebSocket + Actix 消息订阅示例          ║");
    println!("╚══════════════════════════════════════════╝\n");

    info!("开始初始化 WebSocket 客户端...");
    println!("📝 创建 WebSocket 客户端...");

    // 创建 WebSocket Actor
    // 使用环境变量读取代理（WS_PROXY > HTTPS_PROXY > HTTP_PROXY）
    // 如果没有代理，会自动直连
    let ws_actor = WebSocketClient::new("wss://stream.binance.com:9443/ws/btcusdt@depth")
        .with_proxy("http://127.0.0.1:7891")
        .with_reconnect_interval(Duration::from_secs(5))
        .start();

    println!("✓ WebSocket Actor 已启动\n");
    info!("✓ WebSocket Actor 启动成功");

    // 创建连接状态标志
    let is_connected = Arc::new(AtomicBool::new(false));

    // 创建事件处理器 Actor
    let event_handler = EventHandler {}.start();

    // 订阅 WebSocket 事件
    println!("📨 订阅 WebSocket 事件...");
    match ws_actor
        .send(SubscribeToEvents {
            recipient: event_handler.recipient(),
        })
        .await
    {
        Ok(Ok(())) => {
            println!("✓ 已成功订阅事件\n");
            info!("✓ 已订阅 WebSocket 事件");
        }
        Ok(Err(e)) => {
            eprintln!("❌ 订阅失败: {}", e);
            return;
        }
        Err(e) => {
            eprintln!("❌ Actor 邮箱错误: {}", e);
            return;
        }
    }

    // 等待 WebSocket 连接建立（最多等待 5 秒）
    println!("⏳ 等待 WebSocket 连接建立...");
    let mut wait_count = 0;
    while !is_connected.load(Ordering::Relaxed) && wait_count < 50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        wait_count += 1;
    }

    println!("✓ WebSocket 已连接！\n");
    info!("✓ WebSocket 已连接，开始接收消息");

    println!("📡 订阅信息：");
    println!("  • Stream: btcusdt@depth");
    println!("  • 每 100ms 更新深度数据\n");

    println!("═══════════════════════════════════════════");
    println!("开始接收消息（按 Ctrl+C 停止）:\n");
    println!("═══════════════════════════════════════════\n");

    info!("开始接收消息循环");

    // 保持主线程运行
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}
