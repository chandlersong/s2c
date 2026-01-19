# 需求文档：通过WebSocket接收币安加密货币交易数据

## 一、需求概述

在`yu` cargo包中实现通用的IO功能，通过WebSocket实时接收币安交易所的市场数据并存储到可配置的数据库中。这是一个纯技术需求，作为量化交易系统的基础数据层。

## 二、需求详情

### 需求：WebSocket实时数据接收与存储

**用户**：无（纯技术需求，服务于量化交易系统）

**准备的数据和环境**：
- 币安WebSocket API地址：`wss://stream.binance.com:9443/ws`
- 配置文件，包含：
  - 订阅的交易对列表（可配置）
  - 订阅的数据类型（交易、深度）
  - 深度级别和更新频率
  - 数据库类型选择（默认：DuckDB）
  - 各数据类型的保留时间配置
- 数据库连接配置

**数据接收类型**：
2. **交易数据（Trade）**
3. **深度数据（Order Book Depth）**

**流程**：

1. **初始化阶段**
   - 读取配置文件，解析订阅参数
   - 初始化数据库连接
   - 验证币安WebSocket连接可用性

2. **连接建立阶段**
   - 建立WebSocket连接到币安数据流端点
   - 发送订阅请求，根据配置订阅：
     - 多个交易对的交易数据（如：`btcusdt@trade`）
     - 多个交易对的深度数据（如：`btcusdt@depth20@100ms`）
   - 等待订阅确认消息

3. **数据接收阶段**
   - 持续接收WebSocket消息
   - 解析JSON格式的消息
   - 根据消息类型分发到对应的处理器：
     - K线处理器：解析K线数据
     - 交易处理器：解析交易数据
     - 深度处理器：解析深度数据，维护本地快照

4. **数据验证阶段**
   - 检查数据完整性（必填字段）
   - 验证数据格式（价格、数量精度）
   - 检测异常数据（如：价格为0、负数等）
   - 记录数据接收统计信息

5. **数据存储阶段**
   - 通过统一的存储接口（trait）写入数据库
   - 批量写入优化（减少数据库操作次数）
   - 处理存储失败重试
   - 记录存储日志
      - 设计决策：内存缓冲（此前称为 `StorageBuffer`）属于存储/数据库层实现的一部分，负责批量累积与写入策略。
         - WebSocket Handler 只负责解析并将记录交付到统一的存储接口（或发送到存储层缓冲队列），**不在 handler 内维护具体的缓冲数据结构**。
         - 缓冲、批量写入、重试与持久化策略应在 `yu/src/websocket/storage.rs` / `storage_impl.rs` 中实现，并通过 `StorageService` trait 暴露给 handler 使用。

6. **连接维护阶段**
   - 实现心跳保持（定期发送ping消息）
   - 监控连接状态
   - 断线自动重连机制
   - 重连后重新订阅

7. **数据清理阶段**
   - 根据配置的保留时间定期清理历史数据
   - K线数据：按配置保留（如30天）
   - 交易数据：按配置保留（如7天）
   - 深度数据：按配置保留（如1天）

## 三、配置要求

### 必需配置项
- 币安WebSocket端点地址
- 订阅的交易对列表（可配置多个）
- 数据类型开关（K线/交易/深度可独立启用）
- 深度数据级别（5档、10档、20档）和更新频率（100ms或1000ms）
- 数据库类型选择（默认：DuckDB）
- 数据库连接配置
- 各数据类型的保留时间配置

### 可选配置项
- 批量写入大小
- 内存缓冲区大小
- 重连延迟时间
- 日志级别和格式

注：`内存缓冲区大小` 为存储层（`StorageService` / `DuckDBStorage`）的配置项，表示缓冲触发写入的阈值或上限；`handler` 不直接使用该配置来管理内部缓冲。

## 四、验收标准

1. ✅ 能够成功连接到币安WebSocket端点
2. ✅ 能够订阅配置文件中指定的所有交易对和数据类型
3. ✅ K线、交易、深度数据能够正确解析并存储到数据库
4. ✅ 数据库类型可通过配置文件切换，无需修改代码
5. ✅ 断线后能够自动重连并恢复订阅
6. ✅ 能够根据配置定期清理过期数据
7. ✅ 完整的日志记录，包括连接状态、数据接收量、错误信息

## 五、后续扩展

1. 支持其他交易所（OKEX、Huobi等）
2. 支持数据回放功能（历史数据重放）
3. 支持数据质量监控和告警
4. 支持实时数据流导出（Kafka、Redis Streams等）
5. 支持分布式部署和负载均衡

---

**设计决策备注（StorageBuffer 所属）**

- 决策：将 `StorageBuffer`（内存缓冲与批量写入逻辑）移动到存储/数据库模块，不在 WebSocket handler 中维护内部缓冲。
- 原因：存储层更贴近写入语义（批次大小、写入重试、backpressure），并且可以复用到不同的来源（spot/swap/其它交易所）。
- 影响：
   - `BinanceWebSocketDataCollector` 应改为在解析后直接调用存储接口（`StorageService::write_*` 或 `StorageService::enqueue_*`），或注入一个存储缓冲实现。
   - 测试关注点从 handler 的内部缓冲转为 handler 与存储接口的交互（通过 fake 存储实现进行单元测试）。
   - 配置项关于缓冲、batch_size、flush_interval 归入存储模块的配置段。

请在实现阶段遵循该决策，handler 代码应该轻量，仅负责路由和错误处理，所有批量/持久化逻辑交由存储层实现。

---

**设计决策备注（Parser 解耦 / 泛型方案）**

为提升可测试性、可扩展性并支持多种流（spot、swap、option 等），建议对 parser 与 handler 做解耦：

- 目标：使 `BinanceWebSocketDataCollector` 不直接依赖具体的 `SpotMessageParser` 实现，而通过抽象接口注入解析器实现。

- 方案一（推荐，静态分发/高性能）：使用泛型参数

   - 定义 trait：`pub trait SpotParser: Send + Sync + 'static { fn parse(&self, text: &str) -> Result<Option<ParseResult>, YuError>; }`
   - 将 Actor 设为泛型：`struct BinanceWebSocketDataCollector<P: SpotParser> { parser: Arc<P>, ... }`。
   - 提供便捷构造器 `new(config)`（内部构造默认的 `SpotMessageParser`），以及 `new_with_parser(config, Arc<P>)` 用于注入测试用或定制实现。

- 方案二（可选，运行时多态）：trait object

   - 使用 `parser: Arc<dyn SpotParser>` 允许运行时替换解析器，实现更灵活的动态组合，代价是动态分发的微小性能开销。

- 优点：
   - 测试友好：可注入 `FakeParser` 验证 handler 路由/存储调用逻辑；无需在测试中依赖完整 JSON 解析路径。
   - 扩展性：支持不同交易所或流类型，只需实现相同 trait。
   - 关注点分离：handler 负责事件路由与错误处理，解析逻辑集中在 parser 实现中。

- 风险与注意：
   - 泛型会导致不同 parser 类型编译生成不同 actor 类型；若需在运行时以统一类型管理多个 actor，优先考虑 trait object。
   - 需在库中合理 re-export `ParseResult` 类型（或在通用 parser 模块定义通用的 ParseResult），以减少跨模块依赖耦合。

- 实施步骤（最小侵入）：
   1. 新建 `yu/src/websocket/parser.rs`，定义 `SpotParser` trait，并 re-export 解析结果类型 `ParseResult`（或在该模块定义通用的结果类型）。
   2. 在 `yu/src/websocket/binance_spot/spot_parser.rs` 为现有 `SpotMessageParser` 添加 `impl SpotParser for SpotMessageParser`，内部调用 `parse_and_route()`。
   3. 将 `BinanceWebSocketDataCollector` 增加泛型参数 `P: SpotParser`（或将 `parser` 字段改为 `Arc<dyn SpotParser>`），并新增 `new_with_parser(config, Arc<P>)` 构造器；保持 `new(config)` 作为默认快捷构造器。
   4. 修改或添加单元测试：实现 `FakeParser` 用于注入测试，断言 handler 在接收到 `TextMessage` 后调用存储接口或将解析结果交付给存储层。

- 测试策略变化：
   - 单元测试重点由“解析正确性”与“handler 内部缓冲”转为“parser 的解析正确性”（parser 单测）与“handler 与存储层交互”（通过 fake storage 或 fake parser 注入的集成单测）。

此设计使 parser 解耦、提高可替换性，并与此前将缓冲迁移到存储层的决策一致。

## 六、参考资料

- [币安WebSocket API文档](https://binance-docs.github.io/apidocs/spot/en/#websocket-market-streams)
- [币安K线数据格式](https://binance-docs.github.io/apidocs/spot/en/#kline-candlestick-streams)
- [币安交易数据格式](https://binance-docs.github.io/apidocs/spot/en/#trade-streams)
- [币安深度数据格式](https://binance-docs.github.io/apidocs/spot/en/#partial-book-depth-streams)

# 相关设计文件

- [spot websocket基础](binance_websocket_data_collection.design.md)
- [order book维护设计](binance_order_book_maintenance.design.md)
- [spot account设计](account_sync_spot_design.md)

---

**文档版本**：v1.0  
**创建日期**：2026-01-04  
**对应模块**：yu  
**功能分类**：IO  
**状态**：待设计

