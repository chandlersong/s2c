# WebSocketClient + Actix 集成使用指南

## 概述

`WebSocketClient` 现已与 Actix Actor 框架完全集成，支持：
- ✅ 通过环境变量自动读取代理配置
- ✅ 订阅消息模式，将 WebSocket 事件发送给多个 Actor
- ✅ 完整的事件类型：连接、断开、文本消息、二进制消息、重连、错误
- ✅ 从外部 Actor 通过 Recipient 机制发送消息

## 核心特性

### 1. 从环境变量创建客户端

```rust
use yue::websocket::client::WebSocketClient;

// 自动从环境变量读取代理配置
// 优先级: WS_PROXY -> HTTPS_PROXY -> HTTP_PROXY
let client = WebSocketClient::new_with_env_proxy(
    "wss://stream.binance.com:9443/ws/btcusdt@ticker"
);
```

支持的环境变量：
- `WS_PROXY` - WebSocket 专用代理（优先级最高）
- `HTTPS_PROXY` - HTTPS 代理
- `HTTP_PROXY` - HTTP 代理（优先级最低）

### 2. 订阅事件模式

创建一个 EventHandler Actor，订阅 WebSocket 事件：

```rust
use actix::{Actor, Context, Handler};
use yue::websocket::client::WebSocketEvent;

struct EventHandler {
    name: String,
}

impl Actor for EventHandler {
    type Context = Context<Self>;
}

impl Handler<WebSocketEvent> for EventHandler {
    type Result = ();

    fn handle(&mut self, event: WebSocketEvent, _ctx: &mut Self::Context) {
        match event {
            WebSocketEvent::Connected => println!("已连接"),
            WebSocketEvent::Disconnected => println!("已断开"),
            WebSocketEvent::TextMessage(text) => println!("收到: {}", text),
            WebSocketEvent::BinaryMessage(data) => println!("收到二进制: {} 字节", data.len()),
            WebSocketEvent::Reconnecting => println!("重新连接中..."),
            WebSocketEvent::Error(err) => println!("错误: {}", err),
        }
    }
}
```

### 3. 订阅和接收事件

```rust
#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 启动 WebSocket 客户端
    let client_addr = WebSocketClient::new_with_env_proxy(
        "wss://stream.binance.com:9443/ws/btcusdt@ticker"
    ).start();

    // 创建和启动事件处理器
    let handler = EventHandler {
        name: "MyHandler".to_string(),
    };
    let handler_addr = handler.start();

    // 将处理器的 Recipient 发送给客户端，进行订阅
    use yue::websocket::client::SubscribeToEvents;
    
    client_addr.send(SubscribeToEvents {
        recipient: handler_addr.recipient::<WebSocketEvent>(),
    }).await??;

    // 处理器现在会接收所有 WebSocket 事件
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    Ok(())
}
```

### 4. 发送消息给 WebSocket

```rust
use yue::websocket::client::SendTextMessage;

// 发送文本消息
client_addr.send(SendTextMessage {
    text: r#"{"method":"SUBSCRIBE","params":["btcusdt@ticker"],"id":1}"#.to_string(),
}).await??;
```

也可以发送二进制消息：

```rust
use yue::websocket::client::SendBinaryMessage;

client_addr.send(SendBinaryMessage {
    data: vec![1, 2, 3],
}).await??;
```

## 多个订阅者

一个 WebSocketClient 可以有多个订阅者：

```rust
// 创建两个不同的处理器
let handler1 = EventHandler { name: "Handler-1".to_string() };
let handler2 = EventHandler { name: "Handler-2".to_string() };

let h1_addr = handler1.start();
let h2_addr = handler2.start();

// 都订阅同一个客户端
client_addr.send(SubscribeToEvents {
    recipient: h1_addr.recipient::<WebSocketEvent>(),
}).await??;

client_addr.send(SubscribeToEvents {
    recipient: h2_addr.recipient::<WebSocketEvent>(),
}).await??;

// 现在 WebSocket 的所有事件都会发送给这两个处理器
```

## 完整示例

运行完整示例：

```bash
RUST_LOG=info cargo run --example websocket_proxy_example
```

示例包括三种使用场景：
1. **从环境变量读取代理** - 演示 `new_with_env_proxy()`
2. **显式设置代理** - 演示 `with_proxy()` 和多个订阅者
3. **直连 + 发送消息** - 演示如何发送文本消息

## 环境变量配置示例

```bash
# 方式 1: 设置 WS_PROXY（优先级最高）
export WS_PROXY=http://127.0.0.1:7890

# 方式 2: 设置 HTTPS_PROXY
export HTTPS_PROXY=http://proxy.example.com:8080

# 方式 3: 设置 HTTP_PROXY
export HTTP_PROXY=http://fallback.example.com:3128

# 如果环境变量为空字符串，会自动忽略
export WS_PROXY=""  # 这会被视为未设置，继续检查下一个优先级
```

## WebSocketEvent 事件类型

```rust
pub enum WebSocketEvent {
    /// 连接成功
    Connected,
    
    /// 连接断开
    Disconnected,
    
    /// 收到文本消息
    TextMessage(String),
    
    /// 收到二进制消息
    BinaryMessage(Vec<u8>),
    
    /// 重连中
    Reconnecting,
    
    /// 错误
    Error(String),
}
```

## 错误处理

所有 Actix 消息都返回 `Result`：

```rust
match client_addr.send(SendTextMessage {
    text: "subscribe".to_string(),
}).await {
    Ok(Ok(())) => println!("消息已发送"),
    Ok(Err(e)) => println!("发送失败: {}", e),
    Err(e) => println!("Actor 邮箱错误: {}", e),
}
```

## 最佳实践

1. **优先使用环境变量** - 便于部署配置，减少代码耦合
2. **多个订阅者模式** - 一个 WebSocketClient 可服务多个业务逻辑模块
3. **错误处理** - 始终检查返回的 `Result`，特别是在发送消息时
4. **重连策略** - 使用 `with_reconnect_interval()` 自定义重连延迟
5. **日志记录** - 设置 `RUST_LOG=info` 查看 WebSocket 连接状态

## 常见问题

**Q: 如何检查 WebSocket 是否已连接？**
A: 监听 `WebSocketEvent::Connected` 事件

**Q: 发送消息失败如何处理？**
A: 等待 `WebSocketEvent::Connected` 事件后再发送，或在 Handler 中标记连接状态

**Q: 可以同时连接多个 WebSocket 吗？**
A: 是的，创建多个 `WebSocketClient` 实例即可

