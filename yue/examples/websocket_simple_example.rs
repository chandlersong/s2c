/// 最简单的 WebSocketClient 使用示例
///
/// 这个示例展示如何：
/// 1. 从环境变量读取代理配置
/// 2. 创建 Actor 并订阅 WebSocket 事件
/// 3. 处理各种事件类型
use actix::{Actor, Context, Handler};
use log::info;
use yue::websocket::client::{SubscribeToEvents, WebSocketClient, WebSocketEvent};

/// 简单的事件处理器
struct SimpleHandler;

impl Actor for SimpleHandler {
    type Context = Context<Self>;
}

impl Handler<WebSocketEvent> for SimpleHandler {
    type Result = ();

    fn handle(&mut self, event: WebSocketEvent, _ctx: &mut Self::Context) {
        match event {
            WebSocketEvent::Connected(_addr) => {
                info!("✓ WebSocket 已连接");
            }
            WebSocketEvent::TextMessage(text) => {
                info!("📨 收到文本: {}", text.chars().take(50).collect::<String>());
            }
            WebSocketEvent::BinaryMessage(data) => {
                info!("📦 收到二进制: {} 字节", data.len());
            }
            WebSocketEvent::Reconnecting => {
                info!("🔄 重新连接中...");
            }
            WebSocketEvent::Disconnected => {
                info!("✗ WebSocket 已断开");
            }
            WebSocketEvent::Error(err) => {
                info!("❌ 错误: {}", err);
            }
        }
    }
}

#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    env_logger::Builder::from_default_env().filter_level(log::LevelFilter::Info).init();

    info!("========== 简单示例：WebSocket + Actix ==========");
    info!("环境变量代理配置优先级: WS_PROXY -> HTTPS_PROXY -> HTTP_PROXY");

    // 步骤 1: 创建 WebSocket 客户端
    // 自动从环境变量读取代理配置
    let client_addr = WebSocketClient::new_with_env_proxy("wss://stream.binance.com:9443/ws/btcusdt@ticker")
        .with_proxy("http://127.0.0.1:7891")
        .start();

    info!("✓ WebSocket 客户端已启动");

    // 步骤 2: 创建事件处理器
    let handler = SimpleHandler;
    let handler_addr = handler.start();

    info!("✓ 事件处理器已启动");

    // 步骤 3: 订阅事件
    client_addr
        .send(SubscribeToEvents {
            recipient: handler_addr.recipient::<WebSocketEvent>(),
        })
        .await??;

    info!("✓ 事件处理器已订阅");
    info!("等待 WebSocket 事件...\n");

    // 步骤 4: 运行 10 秒
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    info!("\n========== 示例结束 ==========");
    Ok(())
}
