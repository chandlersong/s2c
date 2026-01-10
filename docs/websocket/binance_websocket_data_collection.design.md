# 设计文档：Binance WebSocket Spot 现货数据收集系统（更新版）

## 一、实现功能
- ✅ Spot 现货 Trade / DepthUpdate 数据流订阅
- ✅ 基于现有 `WebSocketClient` (Actix Actor) 的事件驱动处理
- ✅ 复用 `TradeStreamPayload` / `DiffDepthStreamPayload` 数据结构
- ✅ **WsMessageBus 泛型化消息总线**：接收 `WebSocketEvent`，调用 Parser，广播强类型输出（不做路由）
- ✅ 多订阅者：存储、跨所交易、实盘策略等，可独立过滤事件/交易对
- ✅ DuckDB 存储订阅者（Spot stream 数据落库示例）
- ✅ YAML 配置化订阅（Trade/Depth 分开定义，支持全量/最小配置）
- ✅ 缓冲批量写入与定期清理
- ✅ WebSocket 断线自动重连与心跳保活（由 `WebSocketClient` 负责，重连需重放订阅）
- ✅ 二进制入口占位（记录日志/指标，暂未实现）

## 二、技术栈
| 组件 | 技术栈 | 备注 |
|------|--------|------|
| WebSocket 客户端 | `yue::websocket::client::WebSocketClient` | Actix Actor，输出 `WebSocketEvent` |
| 消息总线 | `WsMessageBus<P>` (Actix Actor) | 订阅 `WebSocketEvent`，调用 Parser，广播 `P::Output` |
| 消息解析 | `P: MessageParser`（当前为 `BinanceSpotParser`） | `parse_text` 实现，`parse_binary` 占位未实现 |
| 输出类型 | `P::Output`（当前为 `BinanceSpotWebSocketStreamResponse`） | 订阅者接收强类型 |
| 数据结构 | `TradeStreamPayload` / `DiffDepthStreamPayload` | 现有定义 |
| 运行时 | Actix + Tokio | 现有 |
| 数据库 | DuckDB | 列存储，时间序列友好 |
| 配置 | YAML + serde | 现有 |
| 日志/错误 | `tracing` / `log` + `YueError` | 现有 |

## 三、数据流向（Text/Binary → Parser → 广播）
```
config_all.yaml (交易对大写，如 BTCUSDT/ETHUSDT)
          │
          ▼
AppConfig.binance_websocket
          │
          ▼
WsMessageBus<P> (持 parser, 维护订阅者)
          ▲
          │ SubscribeToEvents
WebSocketClient (连接/收发/重连/心跳)
          │ WebSocketEvent::{TextMessage,BinaryMessage}
          ▼
WsMessageBus<P>
  - parse_text → P::Output (当前: BinanceSpotWebSocketStreamResponse)
  - parse_binary → 未实现，占位日志
  - 广播给订阅者
          ▼
订阅者（示例：StorageSubscriberActor 等）
  - 内部自行过滤 Trade / Depth / Kline
  - 批量写入 DuckDB / 其他处理
```

## 四、架构与初始化顺序（三步）
1) 启动 WsMessageBus（携带 Parser）。
2) 启动订阅者并注册到 Bus。
3) 启动 WebSocketClient，并将其作为消息源注册到 Bus；重连后 WebSocketClient 负责重放订阅。

伪代码：
```rust
let bus = WsMessageBus::new(BinanceSpotParser).start();
let storage = StorageSubscriberActor::new(batch_size, flush_interval_ms).start();

let ws = WebSocketClient::new(ws_url).start();
ws.do_send(SubscribeToEvents { recipient: bus.clone().recipient() });
bus.do_send(Subscribe { subscriber: storage.recipient() });
```

## 五、关键决策与权衡
- Bus 不做路由，仅解析+广播：实现简单、易扩展（多交易所/频道）；缺点是广播开销，订阅者需自过滤。
- Parser 泛型化：便于未来接入 OKEX/更多频道；每类 parser 需独立维护。
- 二进制占位未实现：保留入口，必须日志/指标监控，避免静默丢弃。
- 重连订阅重放：由 WebSocketClient 负责；若遗漏会漏数据，需要在示例/启动逻辑强调。
- 交易对规范：统一使用大写（如 BTCUSDT），与交易所规则一致。

## 六、数据结构
- 输入：`WebSocketEvent::{TextMessage(String), BinaryMessage(Vec<u8>)}`（其余事件对 Bus 非核心）。
- 输出：`P::Output`（当前为 `BinanceSpotWebSocketStreamResponse`），包含 `BinanceSpotEvent::Trade` / `DepthUpdate` / `PartialDepth` 等。
- Parser：
  - `parse_text(&str) -> Result<P::Output, ParseError>`
  - `parse_binary(&[u8]) -> Result<P::Output, ParseError>`（当前返回 `NotImplemented` 并记录日志/指标）

## 七、模块结构
```
yue/src/
├── websocket/
│   ├── ws_message_bus.rs          // WsMessageBus<P>：处理 WebSocketEvent，广播 P::Output
│   └── subscribers/
│       └── storage_subscriber.rs  // 存储订阅者示例，内部过滤 Trade/Depth
└── binance/
    └── parsers/
        └── spot_parser.rs         // BinanceSpotParser：parse_text 实现，parse_binary 占位
```

## 八、交互时序（简版）
启动 → Bus → 注册订阅者 → WebSocketClient 注册为消息源 → Text/Binary → Parser → 广播 Output → 订阅者过滤/写入。

## 九、配置示例
- 交易对保持大写：
```yaml
binance_websocket:
  spot:
    trade:
      enabled: true
      symbols: 
        - BTCUSDT
        - ETHUSDT
      batch_size: 100
      flush_interval_ms: 5000
    depth_update:
      enabled: true
      symbols:
        - BTCUSDT
      batch_size: 50
      flush_interval_ms: 3000
```
- 未定义 `binance_websocket` 时，不启动 WebSocket 收集。

## 十、风险与缓解
- 广播开销：高频行情下 CPU 占用上升；后续可加可选过滤（按符号/事件类型）或分组 Bus。
- 二进制未实现：上线前确认现网是否会下发压缩/二进制；监控 `binary_not_implemented`。
- 订阅重放缺失：在示例/启动流程中明确重放；可增加订阅状态校验。

## 十一、可扩展方向
- 二进制解析（gzip/deflate/Protobuf）并保持相同 `P::Output`。
- 可选路由/过滤层以降低广播风暴。
- 多交易所接入：新增 Parser + 复用 Bus/订阅者模式。

## 十二、实施任务清单（面向开发）
- WsMessageBus
  - [ ] `Handler<WebSocketEvent>`：Text → parse → 广播；Binary → 未实现占位日志/指标。
  - [ ] `Subscribe` 消息：注册订阅者 `Recipient<P::Output>`。
- Parser
  - [ ] `BinanceSpotParser::parse_text` 产出 `BinanceSpotWebSocketStreamResponse`。
  - [ ] `parse_binary` 返回未实现错误并打日志。
- 集成
  - [ ] 启动顺序：Bus → 订阅者 → WebSocketClient；WebSocketClient 重连后重放订阅。
- 订阅者
  - [ ] 存储订阅者：接收 `BinanceSpotWebSocketStreamResponse`，内部过滤 Trade/Depth，批量写入 DuckDB。
- 示例/配置
  - [ ] 示例展示三步初始化与注册；配置交易对大写；未配置则不启动。
- 监控
  - [ ] parse 错误计数、`binary_not_implemented` 计数、关键路径日志。

## 十三、快速指引（TL;DR）
1) 实现 WsMessageBus：接收 `WebSocketEvent`，解析/占位，广播 `P::Output`。
2) 实现 `BinanceSpotParser`。
3) 启动流程：Bus → 订阅者 → WebSocketClient（注册消息源与订阅者）。
4) 订阅者过滤并写入 DuckDB。
5) 跑示例 + 联调配置；关注日志与指标。
