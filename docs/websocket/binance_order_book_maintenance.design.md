# 设计文档：币安订单簿（Order Book）本地维护方案

## 一、目标与范围
- 目标：在 `yue` 包中实现可复用的订单簿维护组件，遵循币安订单簿同步规则，通过 WebSocket 增量事件 + REST 快照维护本地一致视图。
- 范围：Spot 深度流（如 BTCUSDT@depth20@100ms / @depth@100ms）；支持断线重连、缺口检测与重同步；当前实现仅内存维护，不进行数据库/持久化写入。
- 非目标：不负责批量持久化策略（后续可选）；不覆盖除币安外的规则差异（可扩展）。
- 当前实现现状：
  - ✅ 订单簿核心维护逻辑（apply_snapshot、缺口检测）
  - ✅ Actor 架构与订阅者模式
  - ✅ 市场深度裁剪与快照广播
  - ⏳ 未来：配置化、集成到 yu 包、Arrow Flight 查询支持

## 二、契约（Contract）
- 输入：
  - WebSocket 深度事件：包含 U（首个更新ID）、u（最后更新ID）、bids、asks 价位及数量。
  - REST 深度快照：lastUpdateId + 初始 bids/asks（每侧最多5000）。
  - 配置：交易对（大写，如 BTCUSDT）、深度级别/频率、重连/重同步策略、是否导出视图（默认关闭）。
  - 外部查询命令（Arrow Flight）：
    - sql：SQL 字符串。
    - depth：结构化参数 { symbol, market_type(spot|swap), side(bids|asks|both), levels }。
- 输出：
  - 本地订单簿视图（bids/asks：价位→数量），当前更新ID（local_update_id）。
  - 事件式内存通知（可选）：在内存中派发 best N 或增量变更给本进程内的下游策略组件；默认不启用外部存储。
  - Arrow Flight 查询返回：
    - sql：沿用现有 Arrow RecordBatch 流。
    - depth：Arrow 表（symbol, market_type, side, price, qty, update_id, ts），按 levels 与侧别排序返回。
- 成功准则：
  - 事件连续性保证（U..u 覆盖），缺口强制重同步。
  - 断线后能恢复到一致状态。
  - 在 100ms 更新频率下稳定运行。
- 错误模式：
  - 过期事件（u < local_update_id）丢弃。
  - 缺口（U > local_update_id + 1）触发重同步。
  - 快照与事件不对齐需重抓快照。

## 三、核心流程（符合币安规则）
### 订单簿初始化流程
1. OrderBookService 收到未知 symbol 或过期订单簿的深度更新
2. 触发 trigger_init(symbol)，标记该 symbol 为初始化中，缓存该事件到 InitActor
3. InitActor 接收 InitRequest，执行 REST 快照获取：`/api/v3/depth?symbol=PAIR&limit=5000`
4. 缓存初始化期间收到的所有深度事件到 VecDeque
5. 快照获取成功后，创建 OrderBook，应用缓存事件（从 u ≤ lastUpdateId 之后开始应用）
6. 发送 InitComplete 消息回 OrderBookService，携带初始化完成的 OrderBook
7. OrderBookService 保存 OrderBook 到 order_books，清除初始化标记，广播快照给订阅者

### 稳态事件处理流程
1. WebSocket 深度事件到达，OrderBookService.handle_depth_update
2. 若 event.final_update_id < local_update_id：丢弃（过期）
3. 若 event.first_update_id > local_update_id + 1：缺口，触发重同步（回到初始化流程）
4. 对 bids/asks 的每个价位：数量为 0 删除，否则插入/更新
5. 设置 local_update_id = event.final_update_id，更新 last_update_time
6. apply_snapshot 返回 Ok 后，根据 market_depth 裁剪快照，广播给所有订阅者

## 四、数据结构与类型
- 价位与数量采用 `Decimal`（rust_decimal 库）避免浮点误差。
- 内存模型（OrderBook）：
  - `symbol: String`：交易对（大写，如 BTCUSDT）
  - `bids: BTreeMap<Decimal, Decimal>`（降序：最高价在末尾，best_bid 使用 iter().next_back()）
  - `asks: BTreeMap<Decimal, Decimal>`（升序：最低价在前，best_ask 使用 iter().next()）
  - `local_update_id: u64`：最后应用的事件的 final_update_id
  - `last_update_time: u64`：最后更新时间戳（毫秒）
- InitActor 缓存模型：
  - `pending_inits: HashMap<String, VecDeque<DepthUpdateStreamPayload>>`：按 symbol 缓存初始化前的深度事件
  - 缓存上限：当前实现中若超过 100 个事件打印告警
- 消息传递：
  - `OrderBookSnapshotMsg(Arc<OrderBook>)`：订单簿快照（Arc 支持零复制共享）
  - `DepthUpdateStreamPayload`：WebSocket 深度事件（symbol、first_update_id、final_update_id、bids/asks 数组）
- 操作复杂度：
  - 更新单个价位：O(log n)（BTreeMap 插入/删除）
  - 导出前 N 档：O(N)（迭代并收集）
  - 广播快照：O(m)（m 为订阅者数量）

## 五、并发与处理管道
- Actix Actor 架构：
  - `OrderBookService` 主 Actor：
    - 管理所有 symbol 的订单簿快照（HashMap<symbol, Arc<OrderBook>>）
    - 维护订阅者列表，接收 BinanceSpotWebSocketStreamResponse 消息
    - 触发初始化流程，处理初始化完成回调
    - 根据 market_depth 裁剪订单簿快照，广播给所有订阅者
  - `InitActor` 初始化 Actor：
    - 负责 REST 快照获取和事件缓存管理
    - 接收 InitRequest（初始化请求）和 BufferedDepthUpdate（缓存的深度更新）
    - 缓存初始化前的事件，初始化后回放应用
    - 完成后发送 InitComplete 消息回复 OrderBookService
  - `OrderBook` 数据结构（非 Actor）：
    - 单个 symbol 的订单簿维护逻辑
    - apply_snapshot：增量应用深度事件，验证事件连续性
    - 支持克隆与快照，便于在 Arc 中共享
- 内存通知：
  - 通过 actix 的 Recipient 机制在进程内分发快照；订阅者可注册为 Recipient<OrderBookSnapshotMsg>。
- 背压：
  - OrderBookService 仅做内存更新；事件过期自动丢弃；缺口触发重同步。
- 处理流程：
  - WebSocket 解析 → BinanceSpotWebSocketStreamResponse ↓
  - OrderBookService.handle_depth_update ↓
  - 订单簿不存在 → 触发 trigger_init（缓存事件给 InitActor，发 InitRequest）
  - InitActor 获取 REST 快照，应用缓存事件，返回 InitComplete ↓
  - OrderBookService 更新 order_books，广播裁剪后的快照给 subscribers

## 六、断线重连与重同步
- 心跳与状态监控；断线后：
  - 重新建立 wss，重新开始事件缓存与抓快照，回放至一致。
- 缺口检测：任一事件出现 U > local_update_id + 1 立即重同步。
- 指标与告警：重连次数、重同步次数、事件处理延迟（内存统计与日志）。

## 七、权衡
- BTreeMap vs HashMap+堆：
  - 选择 BTreeMap，代码简洁、稳定，满足 5000 档规模与 100ms 频率；如后续需要更快的 top-N 导出，再优化。
- 固定点 vs 浮点：
  - 选择固定点，避免精度误差和比较问题。
- 严格连续性：
  - 任何缺口重同步，保证一致性；代价是极端网络抖动下重同步更频繁。

## 八、模块与接口设计（示例）
- `yue/src/binance/order_book.rs` 中的 Actor 与消息定义：
  - 消息类型：
    - `Subscribe { recipient: Recipient<OrderBookSnapshotMsg> }`：订阅快照
    - `Unsubscribe { recipient_id: usize }`：取消订阅
    - `OrderBookSnapshotMsg(Arc<OrderBook>)`：订单簿快照消息
    - `InitRequest { symbol: String }`：初始化请求
    - `InitComplete { order_book: Arc<OrderBook> }`：初始化完成
    - `BufferedDepthUpdate { symbol: String, update: DepthUpdateStreamPayload }`：缓存的深度更新
  - `OrderBookService` Actor：
    - 字段：subscribers（订阅者列表）、market_depth（广播深度）、order_books（所有订单簿）、initializing（初始化标记）、init_actor（初始化 Actor 地址）
    - 方法：
      - `new()` 创建服务，默认 market_depth=20
      - `with_market_depth(depth)` 配置市场深度
      - `handle_depth_update()` 处理深度更新，触发初始化或更新订单簿
      - `trigger_init()` 触发初始化流程
      - `broadcast_snapshot()` 向所有订阅者广播快照
  - `InitActor` Actor：
    - 字段：pending_inits（初始化中的 symbol 及其缓存）、service_addr（OrderBookService 地址）
    - 处理：InitRequest（获取 REST 快照）、BufferedDepthUpdate（缓存事件）
  - `OrderBook` 结构体（非 Actor）：
    - 字段：symbol、bids/asks（BTreeMap<Decimal, Decimal>）、local_update_id、last_update_time
    - 方法：
      - `new(symbol, depth)` 从 REST 快照创建
      - `apply_snapshot(update)` 应用增量事件，返回 Err(DeprecateError) 当出现缺口
      - `get_sub_order_book(depth)` 裁剪订单簿为指定档位
      - `best_bid()` / `best_ask()` 获取最优价格
      - `bids_count()` / `asks_count()` 获取档位数

## 九、实施步骤（已实现部分）
1. ✅ 在 `yue`：
   - 实现了 `BinanceSpotWebSocketStreamResponse::DepthUpdate` 解析
   - `DepthUpdateStreamPayload` 包含 symbol、first_update_id、final_update_id、bids、asks、update_ts
   - WebSocket 连接与消息分发支持深度流
2. ✅ 在 `yue/src/binance/order_book.rs`：
   - 实现 `OrderBookService` Actor（主维护服务）
   - 实现 `InitActor` 辅助初始化逻辑
   - 实现 `OrderBook` 订单簿数据结构
   - 支持订阅者模式与快照广播
   - 支持市场深度裁剪
3. ⏳ 待完成：
   - Arrow Flight Server 集成（depth 命令入口）
   - 配置结构与 YAML 支持（depth 专属配置）
   - 高层次的 depth 查询 API（如 REST 或 gRPC 包装）

## 十、异常与边界
- 首个事件到来但快照长时间不可用：设置缓存上限与重试/告警策略。
- 价位/数量异常（负数、价格为 0）：丢弃并计数告警。
- 多交易对并发独立；不要共享 `local_update_id`。

## 十一、非功能与运维
- 性能：O(log n) 更新满足 100ms；必要时优化导出。
- 可靠性：自动重连与重同步；全链路日志与指标（内存计数与输出）。
- 安全：遵守速率限制；仅处理公开数据。
- 运维：暴露指标（事件速率、延迟、重连/重同步次数）；通知模块可发送告警。

## 十二、扩展
- 多路由分片：按 symbol hash 将 `BookRouterActor` 分片，`ArrowFlightServer` 按分片路由；提升吞吐与降低单点压力。
- 深度增量查询：除 top-N 外支持返回增量（delta）以便外部重建。
- OKEX 扩展：在 `yue` 增加 OKEX parser 与 connect 注册，复用 `yu` 的维护与查询链路。
