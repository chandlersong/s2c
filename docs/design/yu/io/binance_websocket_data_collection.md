# 设计文档：Binance WebSocket Spot 现货数据收集系统

## 一、实现功能

- ✅ Spot 现货 Trade 和 DepthUpdate 数据流订阅
- ✅ 基于现有 `WebSocketClient` (Actix Actor) 的事件驱动处理
- ✅ 复用 `TradeStreamPayload` 和 `DiffDepthStreamPayload` 结构体
- ✅ DuckDB 数据存储
- ✅ YAML 配置化订阅（Trade 和 Depth 分开定义，支持完整配置和最小配置）
- ✅ 缓冲批量写入和定期数据清理
- ✅ WebSocket 断线自动重连和心跳保活（由 WebSocketClient 负责，重连后需重发订阅命令）
- ✅ 连接管理放在 `yu` 侧，重用现有 `WebSocketClient` 行为

---

## 二、所有技术

| 组件 | 技术栈 | 备注 |
|------|--------|------|
| WebSocket 客户端 | `yue::websocket::client::WebSocketClient` | 现有实现，基于 Actix Actor |
| 消息解析 | `BinanceSpotWebSocketStream::from_text()` | 现有，支持反序列化 |
| 数据结构 | `TradeStreamPayload`, `DiffDepthStreamPayload` | 现有于 `spot_websocket_stream.rs` |
| 异步框架 | Actix Actor + Tokio | 项目现有运行时 |
| 数据库 | DuckDB | 列存储，时间序列优化 |
| 配置管理 | YAML (serde_yaml) + serde | 现有支持 |
| 日志和错误 | `tracing` + `YueError` | 现有工具 |

---

## 三、数据流向

```
┌─────────────────────────────────────────────────────────┐
│ config_all.yaml (新增 binance_websocket 配置)           │
│                                                         │
│ binance_websocket:                                     │
│   spot:                                                │
│     trade:                                             │
│       enabled: true                                    │
│       symbols: [btcusdt, ethusdt]                      │
│       batch_size: 100                                  │
│       flush_interval_ms: 5000                          │
│       retention_days: 7                                │
│     depth_update:                                      │
│       enabled: true                                    │
│       symbols: [btcusdt]                               │
│       batch_size: 50                                   │
│       flush_interval_ms: 3000                          │
│       retention_days: 3                                │
└──────────────────────┬──────────────────────────────────┘
                       │ serde_yaml::from_str()
                       ▼
┌─────────────────────────────────────────────────────────┐
│ AppConfig.binance_websocket                            │
│ ├─ SpotWebSocketStreamConfig                           │
│ │  ├─ trade: StreamConfig                             │
│ │  └─ depth_update: StreamConfig                      │
└──────────────────────┬──────────────────────────────────┘
                       │ ActixActor::start()
                       ▼
┌─────────────────────────────────────────────────────────┐
│ BinanceWebSocketDataCollector (Actix Actor)             │
│ - 管理 WebSocketClient 生命周期                         │
│ - 监听 WebSocketEvent                                  │
└──────────────────────┬──────────────────────────────────┘
                       │ Connected → subscribe_streams()
                       ▼
┌─────────────────────────────────────────────────────────┐
│ WebSocketClient (现有，Actix Actor)                     │
│ - 连接 Binance WebSocket                               │
│ - 发送订阅请求 (JSON)                                   │
│ - 接收原始消息                                          │
│ - 断线自动重连                                          │
└──────────────────────┬──────────────────────────────────┘
                       │ WebSocketEvent::TextMessage(text)
                       ▼
┌─────────────────────────────────────────────────────────┐
│ MessageParser                                           │
│ - BinanceSpotWebSocketStream::from_text()              │
│ - 反序列化为强类型                                      │
└──────────────────────┬──────────────────────────────────┘
                       │
       ┌───────────────┼───────────────┐
       │               │               │
       ▼               ▼               ▼
┌──────────────┐ ┌──────────────┐ ┌─────────────┐
│ Trade        │ │ DepthUpdate  │ │ Other Event │
│ (filtered)   │ │ (filtered)   │ │ (ignored)   │
└──────┬───────┘ └──────┬───────┘ └─────────────┘
       │                │
       └────────┬───────┘
                │ Validator
                ▼
        ┌──────────────────┐
        │ TradeRecord      │
        │ DepthRecord      │
        └────────┬─────────┘
                 │
                 ▼
        ┌──────────────────┐
        │ StorageBuffer    │
        │ (in-memory queue)│
        │ trade_buffer     │
        │ depth_buffer     │
        └────────┬─────────┘
                 │ batch_size OR flush_interval
                 ▼
        ┌──────────────────┐
        │ DuckDBStorage    │
        │ - write_trades() │
        │ - write_depths() │
        │ - cleanup()      │
        └────────┬─────────┘
                 │
                 ▼
        ┌──────────────────┐
        │ DuckDB Database  │
        │ ├─ trades table  │
        │ └─ depths table  │
        └──────────────────┘
```

---

## 四、架构分层（7 个实现阶段）

### 阶段 1: 配置初始化
**职责**：加载配置，初始化系统资源
- 从 `AppConfig.binance_websocket` 读取配置
- 初始化 DuckDB 连接和表结构
- 验证配置有效性（symbols 非空、batch_size 合理等）
- 启动 `BinanceWebSocketDataCollector` Actor（位于 `yu::websocket::binance_spot` 模块）

**关键代码入口**：
```rust
// 在 yu/src/lib.rs 中创建启动函数
pub async fn start_websocket_data_collector() -> Result<()> {
    let config = get_config();
    
    if let Some(ws_config) = &config.binance_websocket {
        // 初始化表
        yu::websocket::binance_spot::init_tables::create_tables()?;
        
        // 创建存储服务
        let storage = Arc::new(DuckDBStorage::new(&config.database)?);
        let buffer = StorageBuffer::new(storage.clone(), ...);
        
        // 启动 Actor
        let collector = BinanceWebSocketDataCollector::new(
            ws_config.spot.clone(),
            buffer,
        ).start();
        
        // 启动清理任务
        let retention = RetentionPolicy::new(storage);
        retention.start_cleanup_task().await;
    }
    
    Ok(())
}

// （可选）yue 的 actix_jobs 可以调用此函数
// 但核心逻辑完全在 yu 中
```

### 阶段 2: 连接建立
**职责**：建立 WebSocket 连接并订阅流
- Actor 启动时收到 `SystemReady` 或初始化信号
- 创建 `WebSocketClient`，设置代理和重连参数（保持在 `yu` 内管理）
- 订阅 WebSocketEvent（通过 `SubscribeToEvents`）
- 监听 `WebSocketEvent::Connected`
- **重连要求**：收到 `Reconnecting/Connected` 后必须重新发送全部订阅命令（参考 `examples/websocket_subscribe_example.rs` 的发送逻辑）

**关键数据**：
```rust
// 订阅请求 JSON 格式
{
  "method": "SUBSCRIBE",
  "params": [
    "btcusdt@trade",
    "ethusdt@trade",
    "btcusdt@depthUpdate@100ms"
  ]
}
```

### 阶段 3: 消息接收
**职责**：接收原始 WebSocket 消息
- 监听 `WebSocketEvent::TextMessage(text)`
- 消息进入 `MessageParser`
- 解析为 `BinanceSpotWebSocketStream` 强类型
- 路由到对应的处理器

**处理流**：
```rust
match BinanceSpotWebSocketStream::from_text(raw_text)? {
    Event(BinanceSpotEvent::Trade(payload)) => { ... }
    Event(BinanceSpotEvent::DepthUpdate(payload)) => { ... }
    _ => { /* 忽略其他事件 */ }
}
```

### 阶段 4: 数据验证
**职责**：验证数据完整性和业务规则
- 检查必填字段（symbol、price、qty 等）
- 验证数据类型和范围（价格 > 0、数量 > 0）
- 检查 symbol 是否在配置允许列表中
- 生成 TradeRecord 或 DepthRecord

**验证规则**：
- TradeStreamPayload：price、qty 必须是有效的数字字符串
- DiffDepthStreamPayload：bids/asks 不为空，final_update_id > first_update_id
- 丢弃无效消息并记录日志

### 阶段 5: 路由和验证
match event {
    BinanceSpotEvent::Trade(payload) => { ... }
    BinanceSpotEvent::DepthUpdate(payload) => { ... }
    _ => { /* 忽略其他事件 */ }
}

### 阶段 6: 数据库写入
**职责**：持久化数据到 DuckDB
- 使用 DuckDB 的 Appender 进行批量插入（高性能）
- 创建 `bn_spot_trade` 和 `bn_spot_depth` 表
- 支持表自动创建（如果不存在）
- 处理重复键冲突（可选：使用 trade_id/final_update_id 去重）

**表结构**：
```sql
CREATE TABLE bn_spot_trade (
    event_time BIGINT NOT NULL,
    symbol VARCHAR NOT NULL,
    trade_id BIGINT NOT NULL PRIMARY KEY,
    price DECIMAL(20, 8) NOT NULL,
    qty DECIMAL(20, 8) NOT NULL,
    buyer_order_id BIGINT,
    seller_order_id BIGINT,
    trade_time BIGINT,
    is_buyer_maker BOOLEAN,
    created_at BIGINT NOT NULL
);
CREATE INDEX idx_bn_spot_trade_symbol_time ON bn_spot_trade(symbol, created_at DESC);

CREATE TABLE bn_spot_depth (
    event_time BIGINT NOT NULL,
    symbol VARCHAR NOT NULL,
    first_update_id BIGINT,
    final_update_id BIGINT NOT NULL PRIMARY KEY,
    prev_final_update_id BIGINT,
    bids_json TEXT,
    asks_json TEXT,
    created_at BIGINT NOT NULL
);
CREATE INDEX idx_bn_spot_depth_symbol_time ON bn_spot_depth(symbol, created_at DESC);
```

### 阶段 7: 数据维护
**职责**：心跳保活、重连、数据清理
- **心跳保活**：由 WebSocketClient 负责（内置 ping/pong）
- **自动重连**：由 WebSocketClient 负责（指数退避）
- **数据清理**：定时任务（例如每日凌晨）删除超过 retention_days 的数据
- **监控告警**：记录错误日志，监控 WebSocket 断线次数

**清理策略**：
```rust
// 每天凌晨 00:00 执行
DELETE FROM bn_spot_trade 
WHERE created_at < now() - INTERVAL '7 days';

DELETE FROM bn_spot_depth 
WHERE created_at < now() - INTERVAL '3 days';
```

---

## 五、关键决策

### 1. 为什么基于现有 WebSocketClient？
- **现有实现**：已有 Actix Actor 支持事件驱动、自动重连、代理配置
- **避免重复**：不需要重新实现连接管理、消息序列化
- **稳定性**：经过测试的生产代码

### 2. 为什么使用缓冲 + 批量写入？
- **性能**：批量写入 DuckDB 比单条写入快 10-100 倍
- **吞吐量**：支持高频消息（每秒数千条）
- **灵活性**：可调整 batch_size 和 flush_interval 平衡延迟和吞吐

### 3. 为什么分离 Trade 和 Depth 配置？
- **灵活性**：不同场景可能只需要某一种数据
- **成本控制**：可以选择性订阅，减少网络带宽
- **数据隔离**：trade 和 depth 特性不同，分表存储便于查询优化

### 4. 为什么支持完整配置和最小配置？
- **开发友好**：最小配置中不存在则不订阅，避免错误配置
- **生产就绪**：完整配置提供所有可选参数
- **向后兼容**：未来易于扩展新配置字段

---

## 六、数据结构定义

### 配置结构体（新增 - 放在 yu 中）

```rust
// yu/src/config.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinanceWebSocketConfig {
    pub spot: Option<SpotWebSocketStreamConfig>,
    // 未来扩展：pub futures: Option<FuturesWebSocketStreamConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotWebSocketStreamConfig {
    pub trade: Option<StreamConfig>,
    pub depth_update: Option<StreamConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    pub enabled: Option<bool>,              // 默认 true
    pub symbols: Vec<String>,               // 币种列表
    pub batch_size: Option<usize>,          // 默认 100
    pub flush_interval_ms: Option<u64>,     // 默认 5000
    pub retention_days: Option<u32>,        // 默认 7
}
```

### 存储记录结构体（新增 - PO 命名规范）

```rust
// yu/src/binance/models/ws_db_po.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecordPo {
    pub event_time: u64,
    pub symbol: String,
    pub trade_id: u64,
    pub price: String,
    pub qty: String,
    pub buyer_order_id: u64,
    pub seller_order_id: u64,
    pub trade_time: u64,
    pub is_buyer_maker: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepthRecordPo {
    pub event_time: u64,
    pub symbol: String,
    pub first_update_id: u64,
    pub final_update_id: u64,
    pub prev_final_update_id: Option<u64>,
    pub bids_json: String,
    pub asks_json: String,
    pub created_at: i64,
}
```

---

## 七、模块文件结构

```
yu/src/
├── binance/
│   ├── models/
│   │   ├── mod.rs
│   │   └── ws_db_po.rs                    # 新增：TradeRecordPo, DepthRecordPo (PO 命名)
│   └── ...
└── websocket/
    ├── mod.rs                              # 新增：导出通用模块
    ├── handler.rs                          # 新增：通用的 WebSocketDataCollector Actor
    ├── validator.rs                        # 新增：通用的数据验证器（已在需求中保留，当前迭代跳过实现）
    ├── storage.rs                          # 新增：通用的 StorageService trait
    ├── storage_impl.rs                     # 新增：DuckDB 实现（通用）
    ├── buffer.rs                           # 新增：通用的 StorageBuffer
    ├── retention.rs                        # 新增：通用的 RetentionPolicy
    └── binance_spot/
        ├── mod.rs
        ├── config.rs                       # 新增：Spot 特定的 StreamConfig
        ├── init_tables.rs                  # 新增：Spot 特定的表初始化
        └── spot_parser.rs                  # 新增：Spot 特定的消息解析器

yue/src/
├── config.rs                              # 修改：添加 BinanceWebSocketConfig
└── binance/
    ├── mod.rs
    └── bn_models/
        ├── mod.rs
        └── spot_websocket_stream.rs       # 现有，复用（TradeStreamPayload 等）
```

---

## 八、风险点和缓解方案

| 风险点 | 影响程度 | 缓解方案 |
|--------|---------|---------|
| WebSocket 断线 | 高 | 由 WebSocketClient 自动重连，Actor 监听断线事件刷新缓冲 |
| 缓冲区溢出 | 高 | 设置合理的 batch_size（100-1000）和 flush_interval（3-10s） |
| 消息解析失败 | 中 | 使用 `map_err` 捕获，记录错误日志，跳过该条消息 |
| DuckDB 并发写 | 中 | 单线程写入，通过 channel 串行化，使用 Appender API |
| 数据重复 | 低 | trade_id 和 final_update_id 作为主键，自动去重 |
| 表创建冲突 | 低 | 使用 `CREATE TABLE IF NOT EXISTS` |
| 配置缺失 | 低 | 使用 `Option<T>`，不存在时不订阅该流 |
| 符号拼写错误 | 低 | 首次连接时验证，失败时记录警告日志 |

---

## 九、备选方案对比

| 方案 | 优点 | 缺点 | 适用场景 |
|------|------|------|---------|
| **DuckDB（推荐）** | 嵌入式、列存储、时间序列优化 | 并发写入有限制 | 当前项目（单机部署） |
| SQLite | 开箱即用、文件存储 | 并发性能差、无列存储 | 数据量小、并发低 |
| ClickHouse | 超高吞吐量、OLAP 优化 | 需要部署、运维复杂 | 超大规模数据（TB+ 级别） |
| PostgreSQL | 强大稳定、扩展性好 | 需要部署、资源消耗多 | 多服务共享数据库 |

**推荐 DuckDB** 的原因：
- 项目已使用 DuckDB（兼容性好）
- 嵌入式部署（无需额外运维）
- 列存储（时间序列查询快）
- 时间序列场景优化（binance 数据天然有时间属性）

---

## 十、可扩展方向

1. **多交易所支持**：增加 OKEX、Huobi WebSocket 收集器（复用同一 Actor 模式）
2. **期货数据**：扩展为 `FuturesWebSocketStreamConfig`，订阅期货 stream
3. **实时告警**：监听大额成交或价格异常波动，发送通知
4. **数据导出**：支持导出到 Parquet、CSV、Arrow Flight
5. **Web 查询接口**：HTTP API 查询 trade 和 depth 数据
6. **实时图表**：WebSocket 推送给前端，实时展示订单簿、K线
7. **性能优化**：支持多 Actor 并行处理（按 symbol 分片）
8. **消息队列**：使用 RabbitMQ/Kafka 解耦收集和存储

---

## 十一、参考配置示例

### config_all.yaml（完整配置，存放在 yu/tests/config_test/）

```yaml
proxyUrl: "http://localhost:7891"
logLevel: "info"
database:
  path: "test.db"

binance_websocket:
  spot:
    trade:
      enabled: true
      symbols:
        - BTCUSDT
        - ETHUSDT
        - BNBUSDT
      batch_size: 100
      flush_interval_ms: 5000
      retention_days: 7
    depth_update:
      enabled: true
      symbols:
        - BTCUSDT
        - ETHUSDT
      batch_size: 50
      flush_interval_ms: 3000
      retention_days: 3
```

### config_min.yaml（最小配置，存放在 yu/tests/config_test/）

```yaml
proxyUrl: "http://localhost:7891"
logLevel: "info"
database:
  path: "test.db"

# binance_websocket 不定义，则不启动 WebSocket 收集
```

---

## 十二、已复用的现有代码

- ✅ `WebSocketClient` - Actix Actor，处理连接、重连、代理
- ✅ `BinanceSpotWebSocketStream::from_text()` - JSON 反序列化
- ✅ `TradeStreamPayload` - 交易数据结构
- ✅ `DiffDepthStreamPayload` - 深度数据结构
- ✅ `AppConfig` 和 `serde_yaml` - 配置加载
- ✅ `DuckDB` - 数据库连接和 API
- ✅ `YueError` - 错误处理
- ✅ `tracing` - 日志系统

---

## 十四、开发规范：PO（Persistent Object）命名约定

### 定义
- **PO（Persistent Object）**：所有需要持久化存储到数据库的数据结构
- **命名规则**：数据库操作相关的结构体统一以 `*Po` 后缀命名（例如：`TradeRecordPo`、`DepthRecordPo`）
- **存储位置**：所有 PO 定义在 `yu/src/binance/models/ws_db_po.rs` 中
- **模块导出**：通过 `yu::binance::models` 导出，供其他模块使用

### 示例

```rust
// yu/src/binance/models/ws_db_po.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecordPo {
    pub event_time: u64,
    pub symbol: String,
    pub trade_id: u64,
    pub price: String,
    pub qty: String,
    pub buyer_order_id: u64,
    pub seller_order_id: u64,
    pub trade_time: u64,
    pub is_buyer_maker: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepthRecordPo {
    pub event_time: u64,
    pub symbol: String,
    pub first_update_id: u64,
    pub final_update_id: u64,
    pub prev_final_update_id: Option<u64>,
    pub bids_json: String,
    pub asks_json: String,
    pub created_at: i64,
}
```

### 使用示例

```rust
// yu/src/websocket/binance_spot/validator.rs
use yu::binance::models::TradeRecordPo;

pub fn validate_trade(payload: &TradeStreamPayload) -> Result<TradeRecordPo> {
    // 验证逻辑
    Ok(TradeRecordPo {
        event_time: payload.event_time,
        // ... 其他字段
    })
}
```

### 优势
- ✅ **明确职责**：通过 PO 后缀清晰表示这是数据库对象
- ✅ **便于维护**：集中管理所有数据库结构，易于追踪数据库变更
- ✅ **避免混淆**：区分业务对象（BO）、数据传输对象（DTO）、持久化对象（PO）
- ✅ **代码可读性**：一眼看出哪些结构体涉及数据库操作

---

## 十六、关键设计原则

1. **充分复用**：基于现有 WebSocket 客户端、数据模型、配置系统；连接由 `yu` 维护，订阅/重连流程对齐 `examples/websocket_subscribe_example.rs`
2. **Actor 模式**：使用 Actix Actor 保证并发安全和错误恢复
3. **异步批处理**：缓冲 + 批量写入，平衡延迟和吞吐
4. **配置驱动**：订阅内容完全从 YAML 配置读取，无需修改代码
5. **职责清晰**：
   - **yu** 库：负责配置、WebSocket 收集、数据存储、启动管理（`start_websocket_data_collector()`）
   - **yue** 库：业务应用层，可选调用 `yu` 的启动函数（或由 `yu` 自行启动）
6. **最小化复杂度**：不重新造轮子，充分利用现有工具和库
