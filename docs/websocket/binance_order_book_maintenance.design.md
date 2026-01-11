# 设计文档：币安订单簿（Order Book）本地维护方案

## 一、目标与范围
- 目标：在 `yu` 包中实现可复用的订单簿维护组件，遵循币安订单簿同步规则，通过 WebSocket 增量事件 + REST 快照维护本地一致视图。
- 范围：Spot 深度流（如 BTCUSDT@depth20@100ms / @depth@100ms）；支持断线重连、缺口检测与重同步；当前阶段仅“内存维护”，不进行数据库/持久化写入。
- 非目标：不负责批量持久化策略（后续可选）；不覆盖除币安外的规则差异（可扩展）。

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
1. 注册到 WebSocket监听连接和重连事件
2. 注册时订阅WsMessageBus的depth事件
2. 抓取 REST 快照：`/api/v3/depth?symbol=PAIR&limit=5000`。
3. 若快照 lastUpdateId ≤ 步骤2的 U，重复抓快照直到满足要求。
4. 丢弃缓存中 u ≤ lastUpdateId 的事件。
5. 将本地订单簿设置为快照，local_update_id = lastUpdateId。
6. 回放剩余缓存事件；进入稳态持续处理新事件。
7. 事件应用规则：
   - 若 event.u < local_update_id：忽略。
   - 若 event.U > local_update_id + 1：判定缺口，丢弃本地簿并重同步（回到步骤1）。
   - 对 bids/asks 的每个价位：数量为 0 删除；否则插入/更新；处理后设 local_update_id = event.u。

## 四、数据结构与类型
- 价位与数量采用固定点整数（基于 tickSize/stepSize）避免浮点误差：`i64` 或 `Decimal` 封装。
- 内存模型：
  - `bids: BTreeMap<Price, Qty>`（降序视图导出时反向迭代）。
  - `asks: BTreeMap<Price, Qty>`（升序）。
  - `local_update_id: u64`。
  - `cache: VecDeque<DepthEvent>` 在快照前缓存与回放。
- 操作复杂度：更新 O(log n)，导出前 N 档 O(N)。

## 五、并发与处理管道
- 单交易对 actor/task：
  - wss reader → parser → synchronizer（订单簿维护） → in-memory notify（可选）。
- 内存通知：
  - 通过 MPSC/广播通道在进程内分发 top-N 视图或增量快照给策略组件；默认关闭。
- 背压：
  - 维护器仅做内存更新与轻量通知；不阻塞 wss 读取；必要时丢弃过期事件并触发重同步。
- actix 全链路：
  - yue → yu：DepthEventMsg（包含 symbol、market_type、U、u、bids、asks、ts）。
  - yu 路由 → OrderBookActor：ApplySnapshot / ApplyEvent。
  - ArrowFlightServer → BookRouterActor：DepthQueryMsg；返回 DepthQueryResult。

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
- `yue/binance/bn_models/spot_websocket_stream.rs中添加depth`
- `yu/src/websocket/order_book_actors.rs`
  - `struct OrderBook { bids: BTreeMap<Price, Qty>, asks: BTreeMap<Price, Qty>, local_update_id: u64 }`
  - `fn apply_snapshot(&mut self, snapshot: Snapshot) -> Result<(), YuError>`
  - `fn apply_event(&mut self, ev: &DepthEvent) -> ApplyResult`
  - `fn export_top(&self, side: Side, n: usize) -> TopView`
- `yu/src/arrow_flight_server.rs`
  - 保持 sql 支持；新增 depth 命令：解析请求为 `DepthQueryMsg`（和 `GetTopView` 对齐），向 `BookRouterActor` 发送并将结果转换为 Arrow 返回。

## 九、实施步骤
1. 在 `yue`：拓展 parser 输出 `market_type`；在 client connect 事件中注册 spot/swap 两类订阅；通过 actix 发送 `DepthEventMsg` 到 `BookRouterActor`。
2. 在 `yu`：实现 `OrderBookActor` 与 `BookRouterActor`，定义消息协议；接入 `Synchronizer` 完成缓存、快照与回放。
3. 在 `yu/arrow_flight_server.rs`：新增 depth 命令入口，调用 `BookRouterActor` 获取视图；保持 sql 路径不变。
4. 配置：在 `yu/config.rs` 增加 depth 配置结构（模仿 trade 配置）：
   - 配置结构：
     ```rust
     #[derive(Deserialize, Debug, Clone)]
     pub struct SpotWebSocketStreamConfig {
         pub trade: Option<SpotTradeStreamConfig>,
         pub depth: Option<SpotDepthStreamConfig>,
     }
     
     #[derive(Deserialize, Debug, Clone)]
     pub struct SpotDepthStreamConfig {
         pub enabled: Option<bool>,
         pub symbols: Vec<String>,           // 交易对列表（大写，如 BTCUSDT）
         pub update_speed: Option<String>,   // 更新频率（100ms / 1000ms），默认 100ms
         pub levels: Option<u32>,            // 深度档位（5/10/20/none），默认 20
         pub snapshot_limit: Option<u32>,    // REST 快照限制（1000/5000），默认 5000
         pub cache_size: Option<usize>,      // 事件缓存上限，默认 1000
         pub max_query_levels: Option<u32>,  // Arrow Flight 查询最大档位，默认 100
     }
     
     impl SpotDepthStreamConfig {
         pub fn update_speed(&self) -> String {
             self.update_speed.clone().unwrap_or("100ms".to_string())
         }
         
         pub fn levels(&self) -> u32 {
             self.levels.unwrap_or(20)
         }
         
         pub fn snapshot_limit(&self) -> u32 {
             self.snapshot_limit.unwrap_or(5000)
         }
         
         pub fn cache_size(&self) -> usize {
             self.cache_size.unwrap_or(1000)
         }
         
         pub fn max_query_levels(&self) -> u32 {
             self.max_query_levels.unwrap_or(100)
         }
     }
     ```
   - YAML 配置示例：
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
           retention_days: 7
         depth:
           enabled: true
           symbols:
             - BTCUSDT
             - ETHUSDT
             - BNBUSDT
           update_speed: "100ms"    # 可选：100ms 或 1000ms
           levels: 20               # 可选：5/10/20/none
           snapshot_limit: 5000     # 可选：1000/5000
           cache_size: 1000         # 可选：事件缓存上限
           max_query_levels: 100    # 可选：查询最大档位
     ```
   - 配置说明：
     - `enabled`：是否启用订单簿维护（默认 false）
     - `symbols`：需要维护的交易对列表（大写）
     - `update_speed`：WebSocket 更新频率（100ms 高频 / 1000ms 低频）
     - `levels`：WebSocket 流档位（5/10/20 部分深度 / none 全量深度）
     - `snapshot_limit`：REST 快照档位上限（1000 或 5000）
     - `cache_size`：同步前事件缓存上限，超过触发告警
     - `max_query_levels`：Arrow Flight depth 查询最大返回档位（防止过大查询）
5. 测试：
   - 单元：事件连续与缺口重同步、数量为 0 删除、路由与查询正确、spot/swap 区分正确。
   - 集成：模拟 yue WS 与 REST；通过 Arrow Flight 触发 depth 查询，验证排序、levels、update_id 一致性；断线重连后查询恢复。
   - 配置测试：类似 trade 配置测试，验证 depth 配置反序列化与默认值。

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
