/// WebSocket 发送订阅消息的示例
///
/// 这个示例展示如何：
/// 1. 连接到币安 WebSocket 组合流端点
/// 2. 发送 SUBSCRIBE 方法的 JSON 消消息来订阅多个交易对流
/// 3. 处理订阅成功/失败的响应
/// 4. 定时发送订阅和取消订阅请求
use actix::{Actor, Addr, Context, Handler};
use log::info;
use serde_json::json;
use std::collections::HashMap;
use yue::binance::bn_json_websocket::{CommandRequest, SPOT_WEBSOCKET};
use yue::binance::bn_models::spot_websocket::BinanceSpotWebsocket;
use yue::websocket::client::{SendTextMessage, SubscribeToEvents, WebSocketClient, WebSocketEvent};

/// 处理订阅消息的事件处理器
struct SubscriptionHandler {
    message_count: usize,
    client_addr: Option<Addr<WebSocketClient>>,
}

impl SubscriptionHandler {
    fn new() -> Self {
        Self {
            message_count: 0,
            client_addr: None,
        }
    }

    fn with_client(mut self, client_addr: Addr<WebSocketClient>) -> Self {
        self.client_addr = Some(client_addr);
        self
    }
}

impl Actor for SubscriptionHandler {
    type Context = Context<Self>;
}

impl Handler<WebSocketEvent> for SubscriptionHandler {
    type Result = ();

    fn handle(&mut self, event: WebSocketEvent, _ctx: &mut Self::Context) {
        match event {
            WebSocketEvent::Connected => {
                info!("✓ WebSocket 已连接，可以发送订阅请求");
                self.message_count = 0;

                // 在连接成功时，自动发送订阅请求
                if let Some(ref client_addr) = self.client_addr {
                    info!("📤 [自动] 在 Connected 事件中发送订阅请求");
                    // JSON RPC 格式的订阅消息
                    let subscribe_msg = json!({
                        "method": "userDataStream.start",
                        "id": 1
                    });

                    let client = client_addr.clone();
                    actix::spawn(async move {
                        if let Err(e) = client
                            .send(SendTextMessage {
                                text: subscribe_msg.to_string(),
                            })
                            .await
                        {
                            info!("❌ 发送订阅消息失败: {:?}", e);
                        }
                    });
                }
            }
            WebSocketEvent::TextMessage(text) => {
                self.message_count += 1;
                // 只打印前几条消息和包含 "result" 或 "error" 的响应消息
                if self.message_count <= 5 || text.contains("result") || text.contains("error") {
                    info!("📨 [消息 #{}] {}", self.message_count, text.chars().take(150).collect::<String>());
                }
                match BinanceSpotWebsocket::from_text(&text) {
                    Ok(response) => match response {
                        BinanceSpotWebsocket::RecentTrades(trades) => {
                            if let Some(first) = trades.result.first() {
                                info!("🧾 最近成交 => price:{} qty:{}", first.price, first.qty);
                            }
                        }
                    },
                    Err(_) => {}
                }
            }
            WebSocketEvent::BinaryMessage(data) => {
                info!("📦 收到二进制消息: {} 字节", data.len());
            }
            WebSocketEvent::Reconnecting => {
                info!("🔄 WebSocket 正在重新连接...");
            }
            WebSocketEvent::Disconnected => {
                info!("✗ WebSocket 已断开，共收到 {} 条消息", self.message_count);
            }
            WebSocketEvent::Error(err) => {
                info!("❌ WebSocket 错误: {}", err);
            }
        }
    }
}

#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    env_logger::Builder::from_default_env().filter_level(log::LevelFilter::Info).init();

    info!("========== WebSocket 订阅消息示例 ==========");
    info!("本示例展示如何通过 WebSocket 发送 SUBSCRIBE/UNSUBSCRIBE 消息");
    info!("连接到币安现货 JSON RPC WebSocket API: {}", SPOT_WEBSOCKET);

    // 步骤 1: 创建 WebSocket 客户端
    // 连接到币安现货 WebSocket JSON RPC 端点
    let client_addr = WebSocketClient::new(SPOT_WEBSOCKET)
        .with_reconnect_interval(std::time::Duration::from_secs(5))
        .with_proxy("http://127.0.0.1:7891")
        .start();

    info!("✓ WebSocket 客户端已启动");

    // 步骤 2: 创建事件处理器
    let handler = SubscriptionHandler::new().with_client(client_addr.clone());
    let handler_addr = handler.start();

    info!("✓ 事件处理器已启动");

    // 步骤 3: 订阅 WebSocket 事件
    client_addr
        .send(SubscribeToEvents {
            recipient: handler_addr.recipient::<WebSocketEvent>(),
        })
        .await??;

    info!("✓ 事件处理器已订阅");

    // 等待连接建立和自动发送的订阅消息
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // 步骤 5: 发送第二个请求 - 获取账户信息
    info!("\n📤 发送请求 #2 - 获取账户信息");
    let mut recent_trades: HashMap<String, String> = HashMap::new();
    recent_trades.insert("symbol".to_string(), "BTCUSDT".to_string());
    recent_trades.insert("limit".to_string(), "1".to_string());
    let trades_command = CommandRequest::new("trades.recent", recent_trades, 2);
    let trade_command_str = serde_json::to_string(&trades_command)?;
    client_addr.send(SendTextMessage { text: trade_command_str }).await??;

    info!("✓ 请求 #2 已发送");

    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    info!("\n========== 示例结束 ==========");
    info!("提示: SPOT_WEBSOCKET 支持的方法包括:");
    info!("  - trades: 成交查询");
    info!("更多方法见: https://binance-docs.github.io/apidocs/spot/cn/");

    Ok(())
}
