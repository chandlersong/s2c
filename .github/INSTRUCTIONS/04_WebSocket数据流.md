# WebSocket 数据流转指南

## 整体架构

```
币安 WebSocket API
        ↓ (连接)
WebSocketClient (Actix Actor)
        ↓ SendTextMessage / SendBinaryMessage
订阅请求 → 币安服务器
        ↓ WebSocketEvent
WsMessageBus<P> (泛型事件总线)
        ↓ 解析 (parse_text / parse_binary)
BinanceSpotWebSocketStreamResponse
        ↓ 广播
多个订阅者
        ↓
存储/处理/转发
```

## 第一步：启动 WebSocketClient

### 创建并启动

```rust
use yue::websocket::client::{WebSocketClient, SubscribeToEvents, SendTextMessage};

// 创建客户端
let client = WebSocketClient::new("wss://stream.binance.com/ws")
    .with_reconnect_interval(Duration::from_secs(5))
    .with_proxy("http://127.0.0.1:7891")  // 可选代理
    .start();  // 启动为Actix Actor

info!("WebSocketClient started");
```

### 客户端功能

- **自动重连**：连接断开后5秒自动重连
- **心跳保活**：Ping/Pong
- **消息缓存**：重连后自动重发订阅请求
- **代理支持**：环境变量或显式设置

## 第二步：创建事件总线和订阅者

### 创建 WsMessageBus

```rust
use yue::websocket::event_bus::{WsMessageBus, WebSocketHandler};
use yue::binance::websocket_handler::BinanceSpotStreamHandler;

// 创建消息总线（自动处理解析和广播）
let bus = WsMessageBus::new(BinanceSpotStreamHandler {})
    .start();

info!("WsMessageBus started");
```

### 创建订阅者 Actor

订阅者是接收解析后数据的业务处理单元。

**示例1：简单打印**

```rust
use actix::{Actor, Context, Handler};
use yue::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamResponse;

pub struct PrintSubscriberActor;

impl Actor for PrintSubscriberActor {
    type Context = Context<Self>;
    
    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("PrintSubscriberActor started");
    }
}

impl Handler<BinanceSpotWebSocketStreamResponse> for PrintSubscriberActor {
    type Result = ();
    
    fn handle(&mut self, msg: BinanceSpotWebSocketStreamResponse, _ctx: &mut Context<Self>) {
        match msg {
            BinanceSpotWebSocketStreamResponse::Trade(trade) => {
                info!("Trade: {} @ {}", trade.symbol, trade.price);
            }
            BinanceSpotWebSocketStreamResponse::Kline(kline) => {
                info!("Kline: {}", kline.symbol);
            }
            _ => {}
        }
    }
}
```

**示例2：存储到数据库**

```rust
use yu::websocket::subscribers::SpotStreamStorageActor;
use yu::duck_db::DBProvider;

let storage = SpotStreamStorageActor::new(
    config.clone(),
    DBProvider::default()
).start();
```

## 第三步：连接各组件

### 注册订阅者到总线

```rust
use yue::websocket::event_bus::Subscribe;

// 注册多个订阅者
bus.do_send(Subscribe {
    subscriber: printer.recipient(),
});

bus.do_send(Subscribe {
    subscriber: storage.recipient(),
});

info!("Subscribers registered to bus");
```

### 注册总线到客户端

```rust
use yue::websocket::client::WebSocketEvent;

client.send(SubscribeToEvents {
    recipient: bus.recipient::<WebSocketEvent>(),
}).await??;

info!("Bus subscribed to WebSocket events");
```

## 第四步：发送订阅命令

### 订阅现货流

```rust
use yue::binance::bn_json_websocket::{StreamCommandRequest, WS_SUBSCRIBE_COMMAND};
use serde_json::to_string;

let command = StreamCommandRequest {
    method: WS_SUBSCRIBE_COMMAND.to_string(),
    params: vec![
        "btcusdt@trade".to_string(),        // 成交流
        "ethusdt@trade".to_string(),
        "btcusdt@depth@100ms".to_string(),  // 深度流，100ms更新
        "ethusdt@kline_1m".to_string(),     // 1分钟K线
    ],
    id: 1,
};

client.send(SendTextMessage::new(
    to_string(&command)?
)).await??;

info!("Subscription command sent");
```

## 数据流详解

### WebSocketEvent 类型

```rust
pub enum WebSocketEvent {
    Connected(Addr<WebSocketClient>),    // 连接成功
    Disconnected,                         // 连接断开
    TextMessage(String),                  // 文本消息（高频）
    BinaryMessage(Vec<u8>),               // 二进制消息
    Reconnecting,                         // 重连中
    Error(String),                        // 错误
}
```

### Parser 接口

```rust
pub trait WebSocketHandler: Send + Sync + Unpin + 'static {
    type Output: ActixMessage<Result = ()> + Clone + Send + Debug;
    
    /// 解析文本消息（JSON）
    fn parse_text(&self, text: &str) -> Result<Self::Output, YueError>;
    
    /// 解析二进制消息（暂未实现）
    fn parse_binary(&self, data: &[u8]) -> Result<Self::Output, YueError>;
    
    /// 连接建立时的回调
    fn on_connect(&self, addr: &Addr<WebSocketClient>) -> Result<(), YueError> {
        Ok(())
    }
}
```

### BinanceSpotStreamHandler 实现

```rust
pub struct BinanceSpotStreamHandler;

impl WebSocketHandler for BinanceSpotStreamHandler {
    type Output = BinanceSpotWebSocketStreamResponse;
    
    fn parse_text(&self, text: &str) -> Result<Self::Output, YueError> {
        // 反序列化JSON
        BinanceSpotWebSocketStreamResponse::from_text(text)
            .map_err(|e| YueError::ParseError(e.to_string()))
    }
}
```

## 广播机制

### WsMessageBus 的广播策略

```rust
impl<P: WebSocketHandler> Handler<WebSocketEvent> for WsMessageBus<P> {
    fn handle(&mut self, event: WebSocketEvent, _ctx: &mut Context<Self>) {
        match event {
            WebSocketEvent::TextMessage(text) => {
                // 1. 解析
                match self.handler.parse_text(&text) {
                    Ok(output) => {
                        // 2. 广播给所有订阅者
                        for subscriber in &self.subscribers {
                            // 先尝试 try_send（非阻塞）
                            if subscriber.try_send(output.clone()).is_err() {
                                // 失败则 do_send（保证送达）
                                subscriber.do_send(output.clone());
                            }
                        }
                    }
                    Err(e) => {
                        error!("Parse failed: {}", e);
                    }
                }
            }
            _ => {}
        }
    }
}
```

### 订阅者邮箱满处理

- `try_send` 失败时自动降级到 `do_send`
- `do_send` 会自动扩展邮箱容量
- 每10秒警告一次，避免日志爆炸

## 完整示例

```rust
#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logger(Some(LevelFilter::Warn), HashMap::new())?;
    
    // 1. 启动客户端
    let client = WebSocketClient::new("wss://stream.binance.com/ws")
        .with_reconnect_interval(Duration::from_secs(5))
        .start();
    
    // 2. 创建总线和订阅者
    let bus = WsMessageBus::new(BinanceSpotStreamHandler {}).start();
    let printer = PrintSubscriberActor.start();
    let storage = SpotStreamStorageActor::new(config, db).start();
    
    // 3. 注册订阅者
    bus.do_send(Subscribe { subscriber: printer.recipient() });
    bus.do_send(Subscribe { subscriber: storage.recipient() });
    
    // 4. 连接
    client.send(SubscribeToEvents {
        recipient: bus.recipient::<WebSocketEvent>(),
    }).await??;
    
    // 5. 发送订阅
    let cmd = StreamCommandRequest {
        method: WS_SUBSCRIBE_COMMAND.to_string(),
        params: vec!["btcusdt@trade".to_string()],
        id: 1,
    };
    client.send(SendTextMessage::new(serde_json::to_string(&cmd)?)).await??;
    
    // 运行
    tokio::time::sleep(Duration::from_secs(600)).await;
    
    Ok(())
}
```

## 配置管理

### config.yaml 中的 WebSocket 配置

```yaml
binance_websocket:
  spot_stream:
    trade:
      enabled: true
      symbols:
        - BTCUSDT
        - ETHUSDT
        - BNBUSDT
      batch_size: 100
      flush_interval_ms: 5000
      retention_days: 7
    
    depth:
      enabled: true
      symbols:
        - BTCUSDT
        - ETHUSDT
      update_speed: "100ms"  # "100ms" 或 "1000ms"
      levels: 20             # 5, 10, 20, 或 none
```

### 加载配置

```rust
let config = get_config();
let ws_config = config.binance_websocket.as_ref().unwrap();
let spot_stream = ws_config.spot_stream.as_ref().unwrap();

let symbols = &spot_stream.trade.as_ref().unwrap().symbols;  // [BTCUSDT, ETHUSDT, ...]
```

## 错误处理和重连

### 自动重连

```rust
impl WebSocketClient {
    pub fn with_reconnect_interval(mut self, interval: Duration) -> Self {
        self.reconnect_interval = interval;
        self
    }
}
```

WebSocket 会在以下情况自动重连：
- 连接超时
- 连接断开
- Ping 无响应

重连后会自动重发所有标记为 `resend_on_reconnect=true` 的消息。

### 监控重连

```rust
match event {
    WebSocketEvent::Reconnecting => {
        warn!("WebSocket reconnecting...");
    }
    WebSocketEvent::Connected(addr) => {
        info!("WebSocket connected");
        // 可在这里重新初始化某些状态
    }
    WebSocketEvent::Error(err) => {
        error!("WebSocket error: {}", err);
    }
    _ => {}
}
```
