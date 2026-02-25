use actix::{Actor, Context, Handler};
use log::info;
use yue::websocket::client_deprecated::{SendTextMessage, SubscribeToEvents, WebSocketClient, WebSocketEvent};

/// 消息处理器 Actor，订阅并处理 WebSocket 事件
struct EventHandler {
    name: String,
}

impl Actor for EventHandler {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("[{}] EventHandler 启动", self.name);
    }
}

impl Handler<WebSocketEvent> for EventHandler {
    type Result = ();

    fn handle(&mut self, event: WebSocketEvent, _ctx: &mut Self::Context) {
        match event {
            WebSocketEvent::Connected(_addr) => {
                info!("[{}] WebSocket 已连接", self.name);
            }
            WebSocketEvent::Disconnected => {
                info!("[{}] WebSocket 已断开", self.name);
            }
            WebSocketEvent::TextMessage(text) => {
                info!("[{}] 收到文本消息: {}", self.name, text.chars().take(100).collect::<String>());
            }
            WebSocketEvent::BinaryMessage(data) => {
                info!("[{}] 收到二进制消息: {} 字节", self.name, data.len());
            }
            WebSocketEvent::Reconnecting => {
                info!("[{}] 正在重新连接...", self.name);
            }
            WebSocketEvent::Error(err) => {
                info!("[{}] 发生错误: {}", self.name, err);
            }
        }
    }
}

#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_default_env().filter_level(log::LevelFilter::Info).init();

    info!("======== WebSocket + Actix 完整示例 ========");

    // 示例 1: 使用环境变量代理
    println!("\n=== 示例 1: 从环境变量读取代理 ===");
    run_example_with_env_proxy().await?;

    // 示例 2: 显式设置代理
    println!("\n=== 示例 2: 显式设置代理 ===");
    run_example_with_explicit_proxy().await?;

    // 示例 3: 直连（无代理）
    println!("\n=== 示例 3: 直连（无代理）===");
    run_example_direct().await?;

    Ok(())
}

/// 示例 1: 从环境变量读取代理配置
async fn run_example_with_env_proxy() -> Result<(), Box<dyn std::error::Error>> {
    let client_addr = WebSocketClient::new_with_env_proxy("wss://stream.binance.com:9443/ws/btcusdt@ticker")
        .with_reconnect_interval(std::time::Duration::from_secs(10))
        .start();

    info!("WebSocket 客户端已启动（环境变量代理）");

    // 创建事件处理器并订阅
    let handler = EventHandler {
        name: "Handler-EnvProxy".to_string(),
    };
    let handler_addr = handler.start();
    let recipient = handler_addr.recipient::<WebSocketEvent>();

    // 发送订阅请求
    client_addr.send(SubscribeToEvents { recipient }).await??;

    info!("EventHandler 已订阅事件");

    // 运行 5 秒后停止
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    info!("示例 1 结束");

    Ok(())
}

/// 示例 2: 显式设置代理
async fn run_example_with_explicit_proxy() -> Result<(), Box<dyn std::error::Error>> {
    let client_addr = WebSocketClient::new("wss://stream.binance.com:9443/ws/btcusdt@ticker")
        .with_proxy("http://127.0.0.1:7890")
        .with_reconnect_interval(std::time::Duration::from_secs(10))
        .start();

    info!("WebSocket 客户端已启动（显式代理）");

    // 创建事件处理器并订阅
    let handler1 = EventHandler {
        name: "Handler-Explicit-1".to_string(),
    };
    let handler1_addr = handler1.start();

    let handler2 = EventHandler {
        name: "Handler-Explicit-2".to_string(),
    };
    let handler2_addr = handler2.start();

    // 两个处理器订阅同一个客户端
    client_addr
        .send(SubscribeToEvents {
            recipient: handler1_addr.recipient::<WebSocketEvent>(),
        })
        .await??;

    client_addr
        .send(SubscribeToEvents {
            recipient: handler2_addr.recipient::<WebSocketEvent>(),
        })
        .await??;

    info!("两个 EventHandlers 已订阅事件");

    // 运行 5 秒后停止
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    info!("示例 2 结束");

    Ok(())
}

/// 示例 3: 直连（无代理）+ 演示发送消息
async fn run_example_direct() -> Result<(), Box<dyn std::error::Error>> {
    let client_addr = WebSocketClient::new("wss://stream.binance.com:9443/ws/btcusdt@ticker")
        .with_reconnect_interval(std::time::Duration::from_secs(10))
        .start();

    info!("WebSocket 客户端已启动（直连）");

    // 创建事件处理器并订阅
    let handler = EventHandler {
        name: "Handler-Direct".to_string(),
    };
    let handler_addr = handler.start();

    client_addr
        .send(SubscribeToEvents {
            recipient: handler_addr.recipient::<WebSocketEvent>(),
        })
        .await??;

    info!("EventHandler 已订阅事件");

    // 等待连接建立后发送消息
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // 发送一条文本消息（演示）
    if let Ok(result) = client_addr
        .send(SendTextMessage::new(r#"{"method":"SUBSCRIBE","params":["btcusdt@ticker"],"id":1}"#))
        .await
    {
        match result {
            Ok(_) => info!("文本消息已发送"),
            Err(e) => info!("发送消息失败: {}", e),
        }
    }

    // 运行 8 秒后停止
    tokio::time::sleep(tokio::time::Duration::from_secs(8)).await;
    info!("示例 3 结束");

    Ok(())
}
