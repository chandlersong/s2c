# 任务拆解：Binance WebSocket Spot 现货数据收集系统

根据设计文档 `docs/design/yu/io/binance_websocket_data_collection.md`，分解为具体实现任务。

---

## 🎯 整体进度

| 优先级 | 完成情况 | 进度 |
|--------|---------|------|
| 1（核心基础） | 完成 1.1，进行中 1.2-1.3 | 1/3 ✅ |
| 2（事件处理） | 未开始 | 0/3 |
| 3（存储） | 未开始 | 0/2 |
| 4（维护） | 未开始 | 0/3 |
| 5（配置） | 未开始 | 0/1 |
| 6（测试） | 未开始 | 0/2 |
| 7（文档） | 未开始 | 0/2 |
| **总体** | **进行中** | **1/16** |

---

## 优先级 1（核心基础）- 配置和表初始化

### 任务 1.1: 配置结构定义 ✅ 完成
- [x] 在 `yu/src/config.rs` 中添加 `BinanceWebSocketConfig` 结构体
- [x] 在 `yu/src/config.rs` 中添加 `SpotWebSocketStreamConfig` 结构体
- [x] 在 `yu/src/config.rs` 中添加 `StreamConfig` 结构体（包含默认值）
- [x] 修改 `AppConfig` 添加 `binance_websocket: Option<BinanceWebSocketConfig>` 字段
- [x] 验证 serde 反序列化工作正常（8 个单元测试全部通过）
- [x] 补充完整的单元测试覆盖

**完成标准：**
- ✅ `yu/tests/config_test/config_all.yaml` 和 `yu/tests/config_test/config_min.yaml` 能被正确解析
- ✅ 所有 8 个单元测试通过
- ✅ 配置结构体实现了 Clone、Debug、Deserialize trait
- ✅ 交易对采用大写格式（BTCUSDT、ETHUSDT 等）

**测试覆盖：**
- ✅ `test_binance_websocket_config_all_deserialization` - 完整配置反序列化
- ✅ `test_binance_websocket_config_min_deserialization` - 最小配置反序列化
- ✅ `test_stream_config_structure` - 结构体字段验证
- ✅ `test_stream_config_clone` - Clone trait 验证
- ✅ `test_spot_websocket_stream_config_clone` - 嵌套结构 Clone 验证
- ✅ `test_binance_websocket_config_debug` - Debug trait 验证
- ✅ `test_stream_config_defaults` - 可选字段验证
- ✅ `test_binance_websocket_config_empty_spot` - 空配置验证

### 任务 1.2: 存储记录结构定义（PO 命名规范）
- [ ] 创建文件 `yu/src/binance/models/ws_db_po.rs`
- [ ] 定义 `TradeRecordPo` 结构体（包含所有字段）
- [ ] 定义 `DepthRecordPo` 结构体（包含所有字段）
- [ ] 为两个结构体实现必要 trait（Debug, Clone, Serialize, Deserialize）
- [ ] 在 `yu/src/binance/models/mod.rs` 中导出这两个结构体
- 完成标准：结构体定义完整，能被其他模块引入（例如 `yu::binance::models::TradeRecordPo`）

### 任务 1.3: DuckDB 表初始化
- [ ] 创建文件 `yu/src/websocket/binance_spot/init_tables.rs`
- [ ] 实现 `create_tables()` 函数，创建 `bn_spot_trade` 表
- [ ] 实现 `create_tables()` 函数，创建 `bn_spot_depth` 表
- [ ] 使用 `CREATE TABLE IF NOT EXISTS` 防止冲突
- [ ] 为 symbol 和 created_at 创建索引（优化查询）
- [ ] 添加单元测试，验证表创建成功
- 完成标准：运行 `create_tables()` 后表结构正确，可以插入数据

---

## 优先级 2（事件处理）- WebSocket 事件监听和解析

### 任务 2.1: Spot 消息解析器
- [ ] 创建文件 `yu/src/websocket/binance_spot/spot_parser.rs`
- [ ] 实现 `SpotMessageParser` 结构体（Spot 特定的解析器）
- [ ] 实现 `parse_and_route()` 函数，调用 `BinanceSpotWebSocketStream::from_text()`
- [ ] 根据消息类型匹配：
  - `BinanceSpotEvent::Trade(payload)` → 返回 `TradeRecordPo`
  - `BinanceSpotEvent::DepthUpdate(payload)` → 返回 `DepthRecordPo`
  - 其他事件 → 忽略并返回 `Ok(None)`
- [ ] 添加错误处理（JSON 解析失败时记录日志）
- [ ] 添加单元测试（使用 Binance 官方示例 JSON）
- 完成标准：能正确解析 trade 和 depth 事件，其他事件被安全忽略

### 任务 2.2: 通用数据验证器
- [ ] 创建文件 `yu/src/websocket/validator.rs`
- [ ] 实现 `Validator` 结构体（通用验证器，与具体交易所无关）
- [ ] 实现 `validate_trade()` 函数：
  - 检查 symbol、price、qty 非空
  - 验证 price 和 qty 是有效的数字字符串
  - 验证 symbol 在配置允许列表中
  - 返回 `TradeRecordPo` 或 `YueError::ValidationError`
- [ ] 实现 `validate_depth()` 函数：
  - 检查 symbol、first_update_id、final_update_id 非空
  - 验证 final_update_id > first_update_id
  - 检查 bids/asks 非空
  - 返回 `DepthRecordPo` 或 `YueError::ValidationError`
- [ ] 添加单元测试（包括正常情况和错误情况）
- 完成标准：能正确验证有效数据，拒绝无效数据

### 任务 2.3: 通用 WebSocket 事件处理 Actor
- [ ] 创建文件 `yu/src/websocket/handler.rs`
- [ ] 定义 `BinanceWebSocketDataCollector` 结构体：
  - `config: SpotWebSocketStreamConfig`
  - `storage_buffer: StorageBuffer`
  - `ws_client: Option<Addr<WebSocketClient>>`
  - `message_count: u64` (统计)
  - `parser: Arc<dyn SpotParser>` (可扩展的 parser 注入)
- [ ] 实现 `Actor` trait：
  - `started()` 初始化 WebSocketClient 和订阅事件（连接管理放在 `yu` 内，复用现有 WebSocketClient）
  - `stopped()` 清理资源（刷新缓冲区）
- [ ] 实现 `Handler<WebSocketEvent>`：
  - `Connected` → 调用 `subscribe_streams()` 发送订阅请求（逻辑参考 `examples/websocket_subscribe_example.rs`）
  - `Reconnecting` / `Connected`（重连后）→ 必须重新发送全部订阅命令
  - `TextMessage(text)` → 调用 `handle_message(text)` 解析和缓冲
  - `Disconnected` → 调用 `flush_buffer()` 刷新缓冲
  - `Error` → 记录日志
- [ ] 实现 `subscribe_streams()` 方法（由具体实现提供，例如 SpotSubscriber）
- [ ] 实现 `handle_message()` 方法：
  - 调用 parser 解析
  - 调用 `Validator::validate_*()` 验证
  - 调用 `storage_buffer.add_trade()` 或 `add_depth()` 缓冲
  - 记录处理计数
- [ ] 添加日志记录（连接、订阅、错误、重连重发）
- [ ] 添加集成测试（mock WebSocketEvent，覆盖重连后重发订阅）
- 完成标准：Actor 能正常启动、接收事件、解析和验证数据，重连后自动重发订阅

---

## 优先级 3（存储）- 缓冲和数据库写入

### 任务 3.1: 通用缓冲队列实现
- [ ] 创建文件 `yu/src/websocket/buffer.rs`
- [ ] 定义 `StorageBuffer` 结构体（通用缓冲，与具体交易所无关）：
  - `trade_buffer: VecDeque<TradeRecordPo>`
  - `depth_buffer: VecDeque<DepthRecordPo>`
  - `batch_size: usize`
  - `flush_interval: Duration`
  - `last_flush: Instant`
  - `storage: Arc<dyn StorageService>`
- [ ] 实现 `new()` 方法：初始化缓冲区和参数
- [ ] 实现 `add_trade()` 异步方法：
  - 将 record 推入 `trade_buffer`
  - 调用 `try_flush()` 检查是否触发刷新
  - 返回 `Result<()>`
- [ ] 实现 `add_depth()` 异步方法（类似）
- [ ] 实现 `try_flush()` 异步方法：
  - 检查是否满足刷新条件（batch_size 或 timeout）
  - 调用 `flush_all()` 执行刷新
  - 更新 `last_flush` 时间戳
- [ ] 实现 `flush_all()` 异步方法：
  - 如果 `trade_buffer` 非空，调用 `storage.write_trades()`
  - 如果 `depth_buffer` 非空，调用 `storage.write_depths()`
  - 清空缓冲区
  - 错误处理：失败时记录日志但不 panic
- [ ] 实现 `flush_on_disconnect()` 异步方法：强制刷新
- [ ] 在 Actor 中使用 `Context::run_interval` 定时触发 `try_flush()`（Actix 无内置缓存，使用 Actor 内存状态作为缓冲）
- [ ] 添加单元测试（验证缓冲和刷新逻辑）
- 完成标准：缓冲区能正确累积数据，按 batch/超时/断开时刷新

### 任务 3.2: 通用存储服务 Trait 和 DuckDB 实现
- [ ] 创建文件 `yu/src/websocket/storage.rs`
- [ ] 定义 `StorageService` trait（通用存储接口）：
  - `async fn write_trades(&self, records: Vec<TradeRecordPo>) -> Result<()>`
  - `async fn write_depths(&self, records: Vec<DepthRecordPo>) -> Result<()>`
  - `async fn cleanup(&self, retention_days: u32) -> Result<()>`
- [ ] 创建文件 `yu/src/websocket/storage_impl.rs`
- [ ] 定义 `DuckDBStorage` 结构体（通用 DuckDB 存储实现）：
  - `conn: Arc<Connection>`
  - `db_path: String`
- [ ] 实现 `new()` 方法：打开 DuckDB 连接
- [ ] 实现 `write_trades()` 方法：
  - 使用 Appender API 批量插入
  - 错误处理（唯一键冲突等）
  - 返回 `Result<()>`
- [ ] 实现 `write_depths()` 方法（类似）
- [ ] 实现 `cleanup()` 方法：
  - 执行 `DELETE FROM bn_spot_trade WHERE created_at < cutoff`
  - 执行 `DELETE FROM bn_spot_depth WHERE created_at < cutoff`
- [ ] 添加单元测试（实际数据库操作）
- 完成标准：能正确插入数据到 DuckDB，实现清理策略

---

## 优先级 4（维护）- 定时任务和启动集成

### 任务 4.1: 通用数据清理策略
- [ ] 创建文件 `yu/src/websocket/retention.rs`
- [ ] 定义 `RetentionPolicy` 结构体（通用清理策略）：
  - `storage: Arc<dyn StorageService>`
  - `trade_retention_days: u32`
  - `depth_retention_days: u32`
- [ ] 实现 `new()` 方法
- [ ] 实现 `start_cleanup_task()` 异步方法：
  - 创建 Tokio 定时任务（每天凌晨执行）
  - 调用 `storage.cleanup()` 删除过期数据
  - 记录清理日志
- [ ] 添加单元测试（模拟时间推进）
- 完成标准：能定时清理过期数据

### 任务 4.2: 模块导出和集成
- [ ] 创建文件 `yu/src/websocket/mod.rs`
- [ ] 导出所有公共通用模块和类型：
  ```rust
  pub mod handler;
  pub mod validator;
  pub mod storage;
  pub mod storage_impl;
  pub mod buffer;
  pub mod retention;
  pub mod binance_spot;
  
  pub use handler::BinanceWebSocketDataCollector;
  pub use validator::Validator;
  pub use storage::StorageService;
  pub use storage_impl::DuckDBStorage;
  pub use buffer::StorageBuffer;
  pub use retention::RetentionPolicy;
  ```
- [ ] 在 `yu/src/lib.rs` 中导出 `websocket` 模块
- [ ] 创建文件 `yu/src/websocket/binance_spot/mod.rs`
- [ ] 导出 Spot 特定模块：
  ```rust
  pub mod config;
  pub mod spot_parser;
  pub mod init_tables;
  
  pub use config::{SpotWebSocketStreamConfig, StreamConfig};
  pub use spot_parser::SpotMessageParser;
  pub use init_tables::create_tables;
  ```
- 完成标准：所有模块可从 `yu::websocket::*` 和 `yu::websocket::binance_spot::*` 导入

### 任务 4.3: 启动集成
- [ ] 在 `yu/src/lib.rs` 中创建启动函数 `start_websocket_data_collector()`：
  - 读取 `yu::get_config()` 获取 `binance_websocket` 配置
  - 如果配置存在，调用 `yu::websocket::binance_spot::init_tables::create_tables()`
  - 创建 `DuckDBStorage` 和 `StorageBuffer`
  - 创建并启动 `BinanceWebSocketDataCollector` Actor（连接由 `yu` 维护，重连后重发订阅）
  - 启动 `RetentionPolicy` 清理任务
  - 记录启动日志
- [ ] 添加错误处理（配置错误、表创建失败等）
- [ ] 添加启动成功的日志提示
- [ ] （可选）在 `yue/src/actix_jobs.rs` 中调用 `yu::start_websocket_data_collector()`
- 完成标准：应用启动时自动启动 WebSocket 收集系统（如果配置存在）

---

## 优先级 5（配置）- 配置文件更新

### 任务 5.1: 更新配置文件
- [ ] 修改 `yu/tests/config_test/config_all.yaml`：
  - 添加 `binance_websocket.spot.trade` 配置（symbols, batch_size 等）
  - 添加 `binance_websocket.spot.depth_update` 配置
  - 参考设计文档中的配置示例
- [ ] 修改 `yu/tests/config_test/config_min.yaml`：
  - 保持最小化（不包含 `binance_websocket` 或为空）
- [ ] 验证两个配置文件能被正确解析
- 完成标准：配置文件语法正确，反序列化无误

---

## 优先级 6（测试）- 单元和集成测试

### 任务 6.1: 单元测试覆盖
- [ ] 为 `parser.rs` 编写单元测试：
  - 测试 trade 消息解析
  - 测试 depth 消息解析
  - 测试无效 JSON 处理
- [ ] 为 `validator.rs` 编写单元测试：
  - 测试有效数据验证
  - 测试无效 symbol 拒绝
  - 测试无效 price/qty 拒绝
- [ ] 为 `buffer.rs` 编写单元测试：
  - 测试缓冲累积
  - 测试 batch_size 触发刷新
  - 测试 timeout 触发刷新
- [ ] 为 `storage_impl.rs` 编写单元测试：
  - 测试数据插入
  - 测试数据查询
  - 测试数据清理

### 任务 6.2: 集成测试
- [ ] 编写集成测试，模拟完整流程：
  1. 加载配置
  2. 初始化 DuckDB
  3. 启动 `BinanceWebSocketDataCollector` Actor
  4. 发送模拟 WebSocketEvent
  5. 验证数据被存储到数据库
- [ ] 测试断线重连场景
- [ ] 测试缓冲溢出场景
- [ ] 为 `handler.rs` 编写集成/单元测试：覆盖 Connected→订阅、Reconnecting→重发订阅、Disconnected→flush 的行为
- 完成标准：集成测试通过，覆盖主要流程

---

## 优先级 7（文档和优化）- 文档和性能优化

### 任务 7.1: 使用文档
- [ ] 创建文件 `docs/yu/io/binance_websocket_data_collection.md`
- [ ] 文档包含：
  - 快速开始（如何配置和启动）
  - API 文档（公共接口和用法）
  - 配置参数说明
  - 故障排查（常见问题和解决方案）
  - 性能调优建议
  - 扩展指南（如何添加新交易所）

### 任务 7.2: 性能优化（可选，后续）
- [ ] 评估当前性能（消息吞吐量、延迟、内存占用）
- [ ] 根据测试结果优化：
  - 调整默认 batch_size 和 flush_interval
  - 考虑使用 arc-swap 优化配置热更新
  - 评估是否需要多线程写入
- [ ] 添加性能基准测试

---

## 完成条件总结

| 优先级 | 完成条件 |
|--------|---------|
| 1 | 配置和表初始化完成，DuckDB 表能正确创建 |
| 2 | WebSocket 事件能正确解析和验证，Actor 能启动，重连后自动重发订阅 |
| 3 | 数据能缓冲并批量写入 DuckDB，使用 Actor 内存缓冲 + run_interval 定时刷新 |
| 4 | 数据清理任务能定时执行，集成到启动流程 |
| 5 | 配置文件更新，符合 YAML 格式 |
| 6 | 单元测试和集成测试通过，覆盖重连重发订阅与断开刷新 |
| 7 | 文档完整，可用于生产环境 |

---

## 检查点

完成以下检查点表示系统可投入使用：

- [ ] `cargo test --lib` 所有测试通过
- [ ] `cargo clippy` 无警告
- [ ] 应用启动时日志显示 "Binance WebSocket Data Collector started"
- [ ] DuckDB 中存在 `bn_spot_trade` 和 `bn_spot_depth` 表
- [ ] 运行 30 分钟后，表中有数据
- [ ] 清理任务定时执行（日志中有 cleanup 记录）
- [ ] 配置缺失时，系统能正常运行（跳过 WebSocket 收集）
