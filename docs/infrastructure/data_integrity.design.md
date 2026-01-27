# 数据完整性校验与自动补拉——设计文档

## 实现的功能
1. 启动时校验与周期校验的统一调度框架
2. 自动补拉与修复验证流程
3. 全局健康状态管理与只读接口暴露
4. 指标、日志、告警的观测与接入
5. 可插拔的校验策略注册表

## 所有的技术
- **异步框架**：Tokio + Actix（Actor 消息传递、定时任务）
- **HTTP/WS 拉取**：复用 yue 层既有 REST 和 WebSocket 封装（无直接调用）
- **数据存储**：复用 yu/li 层 DuckDB/RocksDB 访问封装（无直接调用）
- **配置管理**：基于 config crate，支持环境变量覆盖（YU_* 前缀）
- **观测**：指标通过日志和可选的指标收集器（复用既有集成），告警通过 li::notification
- **并发控制**：信号量/受限队列限流，指数退避策略

## 概要变更（将 Checker 与 ValidationStrategy 结合为独立 CheckActor，由 Supervisor 管理）

为增强隔离性、可扩展性与可观测性，本设计将每个具体的校验策略（实现 `ValidationStrategy` 的对象）封装为一个独立的 actor：`CheckActor`。

- 每个 `CheckActor` 持有自己要执行的 `ValidationStrategy` 实例、调度配置（cron/interval、timeout、重试策略）及要发送结果的订阅者（通常是 `RepairExecutor` 的 recipient）。
- `DataIntegritySupervisor` 负责：创建/销毁/重启 `CheckActor`、聚合各 `CheckActor` 的健康状态、管理 `RepairExecutor` 与全局策略注册表（或将注册表转为 Supervisor 管理的映射），并暴露管理 API（Add/Remove/Update checks、查询 health）。

这样做带来的主要收益：
- 单个策略出问题不会影响其它策略或 Supervisor；
- 每个策略可以独立配置调度与重试参数；
- 更容易对单个策略进行细粒度监控与告警；
- 未来便于做横向扩展（按策略分流到不同进程或节点）。

下文将给出设计细节、消息契约与迁移建议。

## 流程图（更新后的数据流向）

```
                ┌────────────────────┐
                │ 应用启动           │
                └───────┬───────────┘
                        │
                        ▼
            ┌────────────────────────┐
            │ DataIntegritySupervisor│
            └──────────┬────────────┘
                       │
                       │加载配置 + 初始化 HealthState=OK
                       │创建多个 CheckActor（按策略/配置）
                       ▼
    ┌──────────────┬──────────────┬──────────────┐
    │ CheckActor A │ CheckActor B │ CheckActor N │
    │ (strategy A) │ (strategy B) │ (strategy N)  │
    └──────┬───────┴──────┬───────┴──────┬───────┘
           │                  │              │
           │ 初次校验/周期/cron  │ 初次校验/...  │
           ▼                  ▼              ▼
    ┌───────────────────────────────────────────┐
    │             ValidationResult              │
    │             (缺口事件流)                  │
    └──────────────────┬────────────────────────┘
                       │
                       ▼
                 ┌──────────────┐
                 │ RepairExecutor│
                 │  (限流+队列)  │
                 └──────┬───────┘
                        │
                        ▼
                 ┌──────────────┐
                 │ 修复/重算/验证│
                 └──────┬───────┘
                        │
                        ▼
               ┌────────────────────┐
               │ Supervisor 更新状态 │
               └────────────────────┘
```

## CheckActor：契约与行为

- 内部字段（示例）
  - name: String
  - strategy: Arc<dyn ValidationStrategy>
  - schedule: enum { Cron(String), Interval(u64) }
  - timeout_ms: u64
  - retry_policy: BackoffConfig
  - subscriber: Recipient<ValidationResultMsg>

- 主要行为
  - `started()` 时：执行一次初次校验（spawn），并根据 schedule 启动周期/cron 调度。
  - 调度实现：支持两种模式
    - Interval 模式：使用 `ctx.run_interval(Duration::from_millis(interval_ms), ...)` 触发。
    - Cron 模式：使用 cron 表达式（如 Linux cron 语法）解析下一次触发时间点，然后利用 tokio::time::sleep_until 或 tokio_cron_scheduler 等库在异步任务中等待并触发。建议实现细节见下文。
  - 每次执行：通过 `tokio::spawn` 调用 `strategy.validate().await` 并用 `tokio::time::timeout(Duration::from_millis(timeout_ms), ...)` 包裹；通过 `std::panic::AssertUnwindSafe` + `tokio::spawn(async move { std::panic::catch_unwind(...) })` 捕获 panic 并在 `ValidationResult.error` 写入信息。
  - 结果投递：把 `ValidationResultMsg` 发送给 `subscriber`（通常是 `RepairExecutor`），send/ do_send 采用 best-effort。

## Cron 调度：实现建议与注意事项

- 建议使用稳定的 Rust cron 库来解析 cron 表达式，例如 `cron` crate（https://crates.io/crates/cron）或 `cron-parser`。
- 实现方式（推荐）：
  1. 在 `CheckActor` 中存储 `Cron(String)` 文本表达式。
  2. 在 `started()` 或配置变更时解析表达式为 `cron::Schedule`（基于 UTC 或配置时区）。
  3. 在一个独立的 tokio 任务中循环：
     - 使用 `schedule.upcoming(chrono::Utc).next()` 获得下一次触发时间（DateTime<Utc>)。
     - 计算到期的 Instant，并用 `tokio::time::sleep_until()` 等待。
     - 在到期后触发一次策略执行（通过 spawn，遵循超时/捕获逻辑）。
     - 循环查找下一次触发时间。
  4. 当收到 `UpdateConfig` 或 `Stop` 时，取消该任务（保持一个 JoinHandle 并调用 abort）。

- 边界与注意
  - Cron 表达式解析需明确时区（建议使用 UTC 或通过配置显式设置）。
  - 系统时间跳变（NTP、手动调整）会影响调度；使用 `sleep_until` 能较好应对未来时间点，但仍需监控系统时间变动。
  - 当 cron 表达式频繁触发（如每秒），务必在 CheckActor 内实现并发限制或在线程池隔离，避免大量并发任务耗尽资源。

## Supervisor 的职责（扩展）

- 管理 CheckActor 集合：创建、删除、更新配置、查询状态。
- 在 CheckActor 失败/异常时执行重启策略（失败计数 + backoff），或在严重告警时通知外部系统。
- 聚合所有 CheckActor 的健康状态，按照策略级别或全局维度计算 `HealthSnapshot`（OK/DEGRADED/RECOVERING/FAILED）。
- 注入 `RepairExecutor` recipient 给每个 CheckActor，或统一由 Supervisor 做转发。
- 提供管理接口（AddCheck/RemoveCheck/UpdateCheck/ListChecks/GetHealthState/IsCheckerRunning）。

## RepairExecutor（方案 A：中心化 + 策略注册表）

为了满足“每张表修复方式完全不同”又要保持实现简单、资源可控的需求，修复流程采用方案 A：中心化的 RepairExecutor 配合一个可运行时热加载的 RepairStrategy Registry。

核心点：
- RepairExecutor 是系统中负责执行所有修复请求的单一（或少量副本）组件，内部维护一个 worker pool（长度可配），并持有一个 StrategyRegistry（TableId/策略名 -> RepairStrategy factory）。
- RepairStrategy 是每张表（或一组表）具体修复逻辑的实现，必须实现统一的 trait 接口（异步 `repair(ctx)`），并声明其并发特性（是否可并行）和所需参数约定。

运行时流程（简化）：
1. CheckActor 发现缺口，产出 `ValidationResult` 并把 repair_hint（包含 symbol/table/start_ts/end_ts 等）发送给 RepairExecutor。
2. RepairExecutor 根据 `ValidationResult` 构造 `RepairRequest`（生成 `id`、requested_at、填充 idempotency_key 等），并将请求入队到优先级队列（可按 severity/priority 排序）。
3. 出队时，RepairExecutor 根据 `table`(或 `rtype`) 在本地 StrategyRegistry 中查找对应的 RepairStrategy 实现；若未找到，立刻上报失败给 Supervisor。
4. 通过全局令牌桶+per-table semaphore（默认 per-table 并发 1）控制并发，派发到 worker pool 执行：在受保护的任务上下文中调用 `strategy.repair(ctx).await`（加 timeout + catch_unwind）。
5. 执行完成后，RepairExecutor 收集 `RepairResult`（success/failure、affected_rows、details）并上报给 Supervisor；若失败且未超过重试阈值，会按配置的指数退避重新入队。

设计要点与约束：
- 并发控制：RepairExecutor 提供两个层次的并发控制
  - 全局 worker_pool_size（控制总并发）
  - per_table_concurrency（默认 1，支持配置覆盖）
- 幂等性：RepairRequest 应包含 `idempotency_key`（由 Supervisor/CheckActor 或 RepairExecutor 生成）。RepairExecutor 在入队/执行前需做去重（活跃请求与短期历史记录），并将该键传递给策略实现以便策略内部保证幂等。
- 可观测性：每个 RepairRequest/RepairResult 打点（duration_ms、attempts、success/failure、table、strategy、idempotency_key）并暴露指标。
- 策略注册表（StrategyRegistry）：
  - 支持在启动时注册内置策略；也支持运行时热注册/热替换（通过 Supervisor 的管理接口）。
  - 映射键可为 `table` 或 `strategy_name`（优先按 table 匹配，表级配置覆盖策略名级配置）。
- 错误与回退：连续失败（或超过 max_retries）将导致 Supervisor 收到严重告警；对关键表可被 Supervisor 自动提升为 DedicatedActor（见扩展）。

Supervisor 与 RepairExecutor 的职责分工：
- Supervisor：
  - 负责加载配置（table->strategy 映射）、管理 StrategyRegistry（注册/热替换）、维护健康状态与告警、以及管理 CheckActor 的生命周期。
  - 不直接执行修复，但负责在 RepairExecutor 无法处理（如策略缺失或策略连续失败）时做人工告警或临时降级策略（例如把表标记为仅日志记录）。
- RepairExecutor：
  - 负责接收 ValidationResult、封装 RepairRequest、查找策略、执行修复、实现并发/重试/幂等控制并将结果上报 Supervisor。

跨表/事务场景：
- 对于必须跨表原子修复的场景，策略实现需要声明其事务需求（例如需要外部 lock 或数据库事务）。RepairExecutor 在派发时将把该信息传递给策略，策略负责按需获取锁与保证一致性。

扩展与弹性：
- 关键表（高频/复杂修复）可以在运行时被 Supervisor 提升为 DedicatedActor（即方案 2 的专属 actor 模式）以获得更高隔离性；但默认实现以中心化 RepairExecutor 为主，满足大多数表的需求并降低资源消耗。

集成要点（与之前 CheckActor 的交互）：
- CheckActor 只负责发现问题并生成带有 repair_hint 的 ValidationResult；不包含具体修复实现。
- RepairExecutor 对 ValidationResult 做路由、入队并执行策略。执行结果总是上报到 Supervisor，由 Supervisor 做状态汇总和告警决策。

## RepairRequest（简化设计）

为满足轻量实现与快速落地，系统采用一个极简的 `RepairRequest` 消息格式，包含触发修复所需的最小信息：symbol、table、type、以及需要补的时间段。

- 目标：让各表能用各自的修复实现，但消息本身保持简单、可序列化、便于排队与审计。
- 字段（最小集合）：
  - id: u64  // snowflake 唯一 id，由系统生成
  - symbol: Option<String>  // 交易对（如 BTCUSDT），若不适用可为 None；遵循项目规范：大写
  - table: String  // 表名，必须大写
  - rtype: String  // 修复类型（例如 "Backfill"/"Reconcile"/"Resync" 或自定义策略名）
  - start_ts: chrono::DateTime<chrono::Utc>  // 需要补的时间段开始（包含）
  - end_ts: chrono::DateTime<chrono::Utc>    // 需要补的时间段结束（不包含）
  - idempotency_key: Option<String> // 可选，防止重复执行
  - params: Option<serde_json::Value> // 可选，策略特定参数（小而明确定义）

说明与约定：
- 时间窗口语义建议使用 [start_ts, end_ts)（包含起始，不包含结束），以避免重叠歧义。
- `symbol` 仅在与交易/市场相关的表使用；非交易业务可将其置为 None。
- `rtype` 用于路由到具体修复实现（RepairStrategy）；若为自定义策略，使用策略名字符串。
- `idempotency_key` 强烈建议由发起方或 Supervisor 生成（例如："backfill-BTCUSDT-20260101-00-01"），RepairExecutor 应在入队或执行前去重。
- `params` 用于携带小量、非敏感的额外信息（例如主键样例、分片标识、行数上限等），策略内部解析并验证。

集成要点：
- CheckActor 在发现缺口后，应把修复所需的时间窗口和（可选的）symbol 填入 `ValidationResult` 的 repair_hint，RepairExecutor 负责把它封装为 `RepairRequest` 并下发执行。
- RepairExecutor 根据 `table` + `rtype` 在本地 registry 中查找对应的修复实现（或默认实现），执行时遵守幂等与超时策略。
- 为简化实现，默认对同一 `table` 的并发修复限制为 1（可配置覆盖）。

## 迁移步骤（可回滚）
1. 在新分支实现 `CheckActor`（新文件 `check_actor.rs`），并保持旧 `DataIntegrityChecker` 不变（并存实现）。
2. 在 `Supervisor.started()` 中逐步使用 `AddCheck` API 创建少量 CheckActor（而不是一次性替换所有逻辑），并验证 RepairExecutor 的接收。提交阶段性 PR。
3. 运行完整单元与集成测试，并做长时间 smoke test（观察资源占用/任务数）。
4. 当稳定后，把旧的 `DataIntegrityChecker` 替换或移除，并把 Supervisor 默认行为切换为以 CheckActor 为单位。

## 测试与监控建议
- 单元测试：CheckActor 的初次运行、cron 触发（用可控的短 cron 表达式或 mock 时间）、超时、panic 捕获；Supervisor 的 Add/Remove/Restart 流程。
- 集成测试：启动 Supervisor 创建多个 CheckActor（包含 noop/gap/slow 策略），确认 RepairExecutor 收到 ValidationResult 并记录 RepairResult。
- 长时运行测试：1 小时或更长，监控内存/任务数/failed-count。
- 指标与日志：每个 CheckActor 计数器（check_total、check_success、check_failure、check_timeout、last_duration）、Supervisor 聚合指标、RepairExecutor 指标。

## 风险点与缓解（更新）
1. **CheckActor 泛滥导致资源耗尽**
   - 缓解：Supervisor 为 CheckActor 启动设置最大并发阈值、批量启动的速率限制，并监控资源。
2. **cron 解析或时间跳变问题**
   - 缓解：使用标准库/成熟 crate，并在 Scheduler 中对异常情况做重试/回退策略；若发现系统时间跳变，发出告警并短暂降级调度。
3. **跨 actor 状态不一致**
   - 缓解：所有全局状态写入都必须通过 Supervisor 的 handler，实现串行化更新。

## 可能的扩展（同上）
- 多实例部署协调、细粒度修复策略、更多告警渠道与历史追溯等。
