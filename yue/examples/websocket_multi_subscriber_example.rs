/// 多个订阅者示例
///
/// 这个示例展示：
/// 1. 一个 WebSocketClient 可以有多个订阅者
/// 2. 所有订阅者都能收到相同的事件
/// 3. 订阅者可以在运行时动态添加
use actix::{Actor, Context, Handler};
use log::info;
use yue::websocket::client_deprecated::{SubscribeToEvents, WebSocketClient, WebSocketEvent};

/// 第一个处理器 - 统计消息数量
struct CounterHandler {
    name: String,
    count: usize,
}

impl CounterHandler {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            count: 0,
        }
    }
}

impl Actor for CounterHandler {
    type Context = Context<Self>;
}

impl Handler<WebSocketEvent> for CounterHandler {
    type Result = ();

    fn handle(&mut self, event: WebSocketEvent, _ctx: &mut Self::Context) {
        match event {
            WebSocketEvent::Connected(_addr) => {
                info!("[{}] ✓ WebSocket 已连接", self.name);
            }
            WebSocketEvent::TextMessage(_) => {
                self.count += 1;
                info!("[{}] 📨 收到第 {} 条文本消息", self.name, self.count);
            }
            WebSocketEvent::BinaryMessage(_) => {
                self.count += 1;
                info!("[{}] 📦 收到第 {} 条二进制消息", self.name, self.count);
            }
            WebSocketEvent::Reconnecting => {
                info!("[{}] 🔄 重新连接中...", self.name);
            }
            WebSocketEvent::Disconnected => {
                info!("[{}] ✗ WebSocket 已断开", self.name);
            }
            WebSocketEvent::Error(err) => {
                info!("[{}] ❌ 错误: {}", self.name, err);
            }
        }
    }
}

/// 第二个处理器 - 显示消息内容
struct ContentHandler {
    name: String,
}

impl ContentHandler {
    fn new(name: &str) -> Self {
        Self { name: name.to_string() }
    }
}

impl Actor for ContentHandler {
    type Context = Context<Self>;
}

impl Handler<WebSocketEvent> for ContentHandler {
    type Result = ();

    fn handle(&mut self, event: WebSocketEvent, _ctx: &mut Self::Context) {
        match event {
            WebSocketEvent::Connected(_addr) => {
                info!("[{}] 🟢 连接建立", self.name);
            }
            WebSocketEvent::TextMessage(text) => {
                let preview = text.chars().take(80).collect::<String>();
                info!("[{}] 内容预览: {}", self.name, preview);
            }
            WebSocketEvent::BinaryMessage(data) => {
                info!("[{}] 二进制数据: {} 字节", self.name, data.len());
            }
            _ => {}
        }
    }
}

#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    env_logger::Builder::from_default_env().filter_level(log::LevelFilter::Info).init();

    info!("========== 多订阅者示例 ==========");

    // 创建 WebSocket 客户端
    let client_addr = WebSocketClient::new_with_env_proxy("wss://stream.binance.com:9443/ws/btcusdt@ticker")
        .with_proxy("http://127.0.0.1:7891")
        .start();

    info!("✓ WebSocket 客户端已启动");

    // 创建第一个订阅者 - 消息计数器
    let counter1 = CounterHandler::new("计数器1");
    let counter1_addr = counter1.start();

    client_addr
        .send(SubscribeToEvents {
            recipient: counter1_addr.recipient::<WebSocketEvent>(),
        })
        .await??;
    info!("✓ 计数器1 已订阅");

    // 创建第二个订阅者 - 内容显示器
    let content = ContentHandler::new("内容显示");
    let content_addr = content.start();

    client_addr
        .send(SubscribeToEvents {
            recipient: content_addr.recipient::<WebSocketEvent>(),
        })
        .await??;
    info!("✓ 内容显示 已订阅");

    // 等待 3 秒
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    info!("\n--- 动态添加第三个订阅者 ---\n");

    // 动态添加第三个订阅者
    let counter2 = CounterHandler::new("计数器2");
    let counter2_addr = counter2.start();

    client_addr
        .send(SubscribeToEvents {
            recipient: counter2_addr.recipient::<WebSocketEvent>(),
        })
        .await??;
    info!("✓ 计数器2 已订阅");

    // 再运行 5 秒
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    info!("\n========== 示例结束 ==========");
    info!("所有订阅者都收到了相同的事件流");
    Ok(())
}
