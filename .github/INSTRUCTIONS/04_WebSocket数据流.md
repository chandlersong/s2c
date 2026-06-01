# WebSocket 数据流转指南（li 核心抽象 + yu 示例编排）

## 整体架构（核心抽象）

```text
Binance Combined Stream URL
        ↓
WebSocketConnection::run<M>(...)
(li/src/websocket/connection.rs)
        ↓ 读到 Text/Binary
M::from_text / M::from_binary
(li::websocket::models::WebSocketMessage)
        ↓
MessageHandlerTrait<M>::handle_message(...)
(业务侧实现：SpotKlineSaver / SwapKlineSaver)
        ↓
DuckTableTableChannel<KlinePo> -> QueryCommand::Insert
```

> 说明：市场数据/Kline 默认不走 Actix `WebSocketClient + WsMessageBus` 链路。  
> 核心应基于 `li/src/websocket/connection.rs` 的 `WebSocketConnection` / `WebSocketInterface` / `MessageHandlerTrait`；`yu/src/binance/websocket_service.rs` 的 `KlineSubscribeService` 仅为 yu 侧示例编排入口。

---

## 1. 核心组件与职责（以 li 抽象为主）

### 1.1 `WebSocketConnection`（连接管理）
文件：`li/src/websocket/connection.rs`

- 启动入口：`WebSocketConnection::run<M>(url, reconnect_interval, proxy, message_handler)`
- 职责：
  - 建立连接（支持代理）
  - 心跳（60 秒 Ping）
  - 自动重连
  - 接收 Text/Binary 并解析为 `M`
  - 将解析结果交给 `MessageHandlerTrait<M>`
  - 对外暴露 `WebSocketInterface<M>`（事件订阅、命令发送）

### 1.2 `WebSocketInterface<M>`（对外接口）

- `get_event_broadcast()`：订阅连接状态事件
- `get_message_receiver()`：仅当 handler 提供广播 sender 时可用
- `command_sender()`：发送控制命令（关闭/重连/发消息）

### 1.3 `KlineSubscribeService`（yu 侧示例编排）
文件：`yu/src/binance/websocket_service.rs`

- `startup_spot(...)` / `startup_swap(...)`
- 初次订阅：`subscribe_trading_kline(...)`
- symbol 变化后：
  1. 先新建连接
  2. 再关闭旧连接（`close_connection`，带重试）
- 通过 URL `?streams=` 组织订阅，不依赖运行时 SUBSCRIBE 文本命令

---

## 2. 数据与控制流

### 2.1 启动流（yu 侧 Spot Kline 示例）

```text
KlineSubscribeService::startup_spot(...)
  -> start_listen_kline(...)
  -> subscribe_trading_kline(...)
  -> WebSocketConnection::run::<BinanceSpotWebSocketStreamWrapper>(...)
```

`subscribe_trading_kline` 会构造：

```text
let final_url = format!("{}?streams={}", ws_url, KlineSubscribeService::compose_kline_url(symbols, interval));
```

其中 stream 形如：`btcusdt@kline_5m/ethusdt@kline_5m`。

### 2.2 消息处理流

在 `WebSocketConnection` 内部：

- `WsMessage::Text` -> `M::from_text(&text)` -> `handler.handle_message(&m).await`
- `WsMessage::Binary` -> `M::from_binary(data)` -> `handler.handle_message(&m).await`

Kline 落库处理由业务 handler 实现：

- `SpotKlineSaver`：处理 `BinanceSpotWebSocketStreamWrapper`
- `SwapKlineSaver`：处理 `BinanceSwapWebSocketStreamWrapper`

仅 Kline 收盘数据写库（`is_closed` / `is_close`）。

---

## 3. 连接事件与命令

### 3.1 `WebSocketEvent`

定义于 `li/src/websocket/connection.rs`：

```rust
pub enum WebSocketEvent {
    Connected(UnboundedSender<CommandMessage>),
    Disconnected,
    Reconnecting,
    Error(String),
}
```

用途：仅连接状态通知，不传递逐条行情 payload。

### 3.2 `CommandMessage`

```rust
pub enum CommandMessage {
    Connection(ConnectionAction),
    ToServer(ToServerMessage),
}
```

常见控制命令：

```text
CommandMessage::Connection(ConnectionAction::Close)
```

用于旧连接下线、平滑切换。

---

## 4. yu 侧最小使用示例（Spot）

```text
use yu::binance::websocket_service::KlineSubscribeService;

// db: DuckTableTableChannel<KlinePo>
// watcher: BinanceDashboardWatcher
KlineSubscribeService::startup_spot(
    db,
    watcher,
    proxy,      // Option<String>
    interval,   // yue::models::HistoryInterval
).await?;
```

## 7. 实施约束（文档口径）

1. 市场数据默认描述统一为：`WebSocketConnection` / `WebSocketInterface` / `MessageHandlerTrait`（li 核心抽象）；`KlineSubscribeService` 仅作 yu 侧编排示例。
2. 事件定义统一引用 `WebSocketEvent`（`Connected` / `Disconnected` / `Reconnecting` / `Error`）。
3. 不再将 `WsMessageBus`、`SubscribeToEvents`、`SendTextMessage` 作为默认范式。
4. 示例符号名保持与当前代码一致，不引入新抽象名。
