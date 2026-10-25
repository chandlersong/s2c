---
name: vaildate-sync-data
description: 检查同步模型创建、服务端同步和客户端同步技能的工作结果，验证数据链路、字段映射、持久化与运行接线是否完整一致。
---

# 验证同步数据链路

## 目标

检查 `create-sync-model-by-vo`、`server-side-sync-entity` 和 `client-side-sync-entity` 三个 skill 的工作结果，
确认目标数据从来源到服务端、协议、客户端和数据库的链路完整、语义一致且已接入实际运行入口。以代码和运行入口为依据，
不因文件、类型、trait 或方法已经创建就判定完成。

本 skill 将检查分成四类：

1. **核心类与生产组装（优先）**：核对主要类职责、依赖注入、构造顺序、注册关系及真实入口。
2. **同步模型与协议**：VO、双方 PO、protobuf、表结构、repository。
3. **服务端和客户端同步**：History/Instrument service、job 调度、ID 映射、订阅、回补与落库。
4. **端到端集成与验证**：两端契约、测试、示例、格式化、编译和启动接线。

默认只检查并报告问题，不擅自修改实现。用户明确要求修复时，再对确认的问题进行最小修改并运行相关验证。

## 优先检查：核心类与生产组装

先根据 `create-sync-model-by-vo`、`server-side-sync-entity` 和 `client-side-sync-entity` 的主要类职责及参考实现，
对照目标实现的类、构造方式和生产组装。此项是判定链路是否按项目模式接入的首要依据；字段逐项正确、单元测试通过或 example 可运行，
都不能替代核心类和生产入口的核验。

### 1. 模型类

- 沿 `VO → 服务端 PO → protobuf → 客户端 PO` 确认每个边界由合适的模型承载，转换责任清楚；schema、repository 和表初始化
  与对应 PO 相匹配。
- 参考 `create-sync-model-by-vo` 中指定的相邻实现，核对模型是否放在对应 crate/模块，客户端 PO 是否能使用本地 instrument ID。
- 不要求类名和文件与范例完全相同；若结构不同，确认其职责、依赖方向和端到端映射等价，不重复引入同一职责的类。

### 2. 服务端类与组装

- 参考 `server-side-sync-entity` 的 `PolyMarketSyncHistoryService`、`PolyMarketSyncInstrumentService`、
  `start_polymarket_sync_series_job` 和 `yu/src/bin/yu_sync_server.rs`。
- 核对 History service 是否通过生产构造函数获得所需 repository/API 和 `history_tx`，并负责 PO 到 `HistoryEntry` 的转换与发送；
  Instrument service 是否使用正确 repository、提供目标 instrument 和支持的 History service。
- 核对 job 是否将相同的共享依赖传给相关 service，注册正确的 `HistoryType`，按业务需要执行 instrument 初始化、history 初始化
  及实时/周期任务；再确认 `yu_sync_server` 按配置启用 job 并把 Instrument service 注册到 `YuSyncServer`。
- 按数据来源判断调度方式：REST 拉取由 job 调用，WebSocket 实时处理由消息驱动；不可只因范例有某个方法就强制不适用的实现。

### 3. 客户端类与组装

- 参考 `client-side-sync-entity` 的 `PolymarketClientInstrumentService`、`PolymarketClientHistoryService`、
  `SyncClientService` 和 `yu/src/bin/yu_sync_client.rs`。
- 核对 Instrument service 的生产构造是否建立共享的服务端 ID → 本地 instrument ID 映射，并创建、注册对应 History service；
  History service 的生产构造是否获得正确的 repository、batch insert、gRPC manager 和所需映射。
- 核对 `SyncClientService` 是否合并各 History service 并按 `InstrumentType` 注册 Instrument service；实时消息是否按
  `HistoryType` 分发。最后追踪客户端入口的表初始化、服务构造、订阅、instrument 初始同步和对账调度。
- 核对远端 ID 到本地 instrument 的解析是否发生在落库前；不能只因 service 已构造或映射缓存已声明，就判定接线正确。

### 4. 组装结论

- 按“核心类职责 → 构造依赖 → service 注册/分发 → job 与入口调用”顺序追踪，不从单个类型或转换函数直接推断链路完整。
- 允许与范例不同的类名、文件划分和调用顺序，但需说明其等价实现依据；确有职责缺失、依赖未注入、注册遗漏或生产入口未调用时，
  按缺陷报告。
- 报告结果时，先给出核心类及组装是否符合三个同步 skill 的结论，再列数据字段、schema、边界语义和测试方面的问题。

## 检查前准备

- 确认目标业务对象、交易所、HistoryType、InstrumentType、对应 VO/PO，以及用户要求检查的 skill 或实现范围。
- 检查工作区状态和相关改动；保留用户已有的未提交修改，不覆盖或还原无关改动。
- 从用户需求、三个同步 skill、相邻实现和生产入口确定预期行为。若检查范围只覆盖其中一个 skill，仍需核对必要的上下游契约，但不扩展为全仓审计。
- 只阅读目标链路相关文件；不读取、输出或修改凭证及私有环境配置。
- 对照实际生产代码验证；测试、example 或占位类型不能替代生产接线。

## 一、同步模型与协议

完成“核心类与生产组装”检查后，再参考 `create-sync-model-by-vo`，逐字段核对从上游到客户端的结构和持久化定义。

### 1. 来源与字段语义

- 确认指定 VO 的定义和生产点，核实每个同步字段确实来自该 VO 或其上游，不凭字段名猜测业务含义。
- 逐字段比对 VO、服务端 PO、protobuf、客户端 PO、服务端 DDL、客户端 DDL 和实际写入 SQL。
- 核对字段类型、名称/语义、`Option`/nullable、默认值、Decimal 精度、枚举值及序列化表示；所有有意转换都应有明确依据。
- 核对时间字段的单位（秒、毫秒、微秒或纳秒）、时区和精度，确认转换没有截断、重复乘除或把时区时间当作无时区时间。
- 确认现货/合约等多个上游形态若共用模型时，来源区分字段得到保留，各来源均有对应映射。

### 2. ID、业务键与关联

- 分别识别服务端 PO ID、客户端 PO ID、服务端 instrument ID、客户端本地 instrument ID 和业务唯一键。
- 检查 protobuf 携带的 ID 在接收端被解释为正确的 ID；不得把服务端 ID 直接当作客户端本地数据库主键或外键写入。
- 核实业务唯一键来自真实业务语义；随机生成 ID 不能代替 upsert/冲突处理所需的业务唯一键。
- 涉及 instrument 外键时，确认数据写入前完成远端 ID 到本地 instrument ID 的解析，并处理映射未命中的情形。

### 3. Protobuf 与数据库定义

- 检查 `history_data.proto` 中消息、字段号、枚举、`oneof` 和请求/响应关联；确认分发代码也识别新类型。
- 不复用已占用的 protobuf 字段号，不随意删除、改号或改变已有字段语义。
- 比对双方 DDL、PO、表枚举/注册、`ALL_CLIENT_TABLES` 或对应初始化入口，确认目标表可在真实运行路径中创建。
- 比对 repository 的查询/插入 SQL 与 DDL：列名、列顺序、占位参数、类型、NULL 编码、唯一约束和 `ON CONFLICT` 目标必须一致。
- 检查 SQL 值使用绑定参数；表名等标识符必须来自静态受控定义。
- 检查范围查询的开闭区间和稳定排序；确认写入具备所需幂等性，冲突不会静默丢弃有效数据。
- repository 若只是后续工作用的占位 trait，应确认本次需求不要求其方法；若实现声称已完成持久化，则必须追到实际 SQL 和调用点。

## 二、服务端同步

参考 `server-side-sync-entity`，检查 History 和 Instrument 两条服务端路径，并沿 job 追踪实际运行接线。

### 1. History repository 与 service

- 确认 repository trait 覆盖 service 实际需要的查询/写入操作，SQL 与表定义相符，错误按 crate 约定传播。
- 检查 service 是否接收 `history_tx: Sender<HistoryEntry>`；`broadcast_message` 是否将正确的 PO 转换为对应 HistoryEntry 并发送。
- 检查 `for_production` 的依赖、参数和生产构造路径，确认其使用目标 repository/API，而不是测试 mock 或不相关 service。
- 按业务需要核对以下方法：
  - `initial_history_data`：通过已有 REST API 或既定来源初始化历史数据；初始化数据直接落库，不应误当作实时消息广播。若无法取得历史数据，应遵循 skill 的询问/说明要求，不得臆造 API。
  - `sync_all`：获取最新数据并广播给客户端；REST 实现应有周期调度，WebSocket 实现应由连接消息驱动而非再增加重复的周期拉取。
  - `check_history_data`：使用现有二分查找/缺口检测机制检查本地数据；按初始化逻辑补齐缺失数据，并将修复数据发送给客户端。
- 对不适用或不可实现的方法，检查是否有清晰原因和符合项目规范的处理；不能以空实现、吞错或成功形状的默认值掩盖缺失。

### 2. Instrument repository 与 service

- 检查 `list_all_instruments` 是否返回目标业务需要的完整 PO 列表。
- 检查 `for_production` 是否注入正确的 repository 和依赖。
- 检查 `live_instruments` 缓存是否能在相关运行环境共享，读写是否受一致的锁/并发机制保护。
- 检查 `support_history_list` 是否返回目标 instrument 支持的历史类型，优先复用外部注入的支持列表。
- 若实现 `sync_instruments`，确认它能更新持久化数据及 `live_instruments` 缓存；若本业务不需要此功能，不能仅因通用示例存在就强行扩展。
- 确认 service 直接依赖 repository 接口，不通过另一个 service 间接访问 repository。

### 3. Job 调度和服务器入口

- 在组装 job 中确认调用顺序：
  1. 获取/初始化 instruments，并在启动时执行一次 `sync_instruments`（若目标业务实现该方法）。
  2. 调用 `list_all_instruments`，使 History 初始化使用最新 instrument 集合。
  3. 在 Instrument 初始化之后调用 `initial_history_data`。
  4. 按业务来源启动实时 WebSocket 或 REST 周期同步，不重复触发同一数据源。
- 对适用的 REST History 路径，确认 `sync_all` 每小时调度一次，并按既有约定随机安排在整点后 90 秒内。
- 对适用的 History 完整性检查，确认每 6 小时运行一次，分钟值不是整点且不是 5 的倍数。
- 对适用的 Instrument 刷新，确认启动时运行一次、之后每 6 小时运行一次，分钟和秒不是整点或 5 的倍数。
- 检查 timer/cron 的字段数量、时区和单位，避免表达式看似合理但实际频率错误。
- 确认 `yu/src/bin/yu_sync_server.rs` 注册目标服务/job，且配置开关能控制是否启用；example 应展示必要的显式调用和生产组装方式。
- 用户要求的链路必须在真实入口启用，不能只存在于 example 或未调用的 `sync_job.rs`。

## 三、客户端同步

参考 `client-side-sync-entity`，检查 instrument 初始化、history 回补、实时订阅及完整性对账。

### 1. Instrument service

- 检查 `for_production` 是否创建服务端 ID 到本地 instrument 的共享映射。
- 确认该 instrument service 创建并注册目标 HistoryType 对应的 history services。
- 检查 `initial_data` 是否遍历注册的 history services，并用正确 instrument 和 `end_ms` 初始化。
- 检查 `supported_history_services` 返回的服务集合是否用于 `SyncClientService` 的实际注册，避免存在 service 却未分发。

### 2. History service 与历史回补

- 检查 `for_production` 是否创建正确的 repository、batch insert、gRPC manager、`BinarySearchDS`，并设置正确的 history interval 及共享 ID 映射。
- 检查 `initial_data` 是否：
  1. 按服务端 instrument ID 查找本地 instrument。
  2. 不存在时创建正确的本地 PO，并更新远端 ID 到本地 ID 缓存。
  3. 查询本地最新 history 时间；无记录时从 instrument 的 `start_ms` 开始，有记录时从最新时间开始。
  4. 调用 `backfill_history` 同步至传入的 `end_ms`。
- 检查 `backfill_history` 的请求范围是否为 `(start_timestamp, end_timestamp]`。
- 检查每条返回记录的 `history_type`、payload variant 和服务端 instrument ID；确认转换后使用客户端本地 instrument ID 落库。
- 检查 `batch insert` 的列顺序、NULL 处理、批次 flush、冲突键及错误处理。
- 检查 `insert_history` 是否验证 history type、instrument type、payload variant；优先查 ID 映射缓存，未命中时查询 repository 并更新缓存。
- 若本地 instrument 不存在，按既有日志/错误策略明确处理；不得把服务端 ID 当成本地 ID继续插入。

### 3. 缺口对账、订阅与入口

- 检查 `reconcile_living_instrument_with_server` 是否按 history interval 对齐起止时间，并从 repository 获取仍在运行的本地 instruments。
- 确认对每个 instrument 调用 `binary_search_gap`；对 `MissingData` 使用正确区间调用 `backfill_history`。
- 检查 `SyncClientService` 是否按 `InstrumentType` 找到对应 instrument service，按每个 entry 的 `HistoryType` 分发到正确 history service。
- 检查订阅流结束、断线或重连后是否按现有逻辑重新订阅，且不会造成遗漏或重复落库问题。
- 检查 `yu/src/bin/yu_sync_client.rs` 是否在真实启动路径中初始化客户端表、创建 `SyncClientService` 和 gRPC manager、启动订阅、获取服务端 instruments 并执行初始同步，再按既有调度启动 living instrument 对账。
- 检查客户端 example 是否体现目标 instrument 的查找及初始化/对账路径；example 不能替代入口接线。

## 四、端到端集成与验证

### 1. 端到端追踪

- 从数据来源追到服务端 PO/表，再追到 protobuf 发送、客户端接收/分发、客户端 ID 映射、batch insert 和最终表。
- 同一字段在每个边界都要核对值、类型、NULL、时间精度和单位；不能只检查单个转换函数。
- 同一 HistoryType/InstrumentType 在 protobuf、service 注册表、调度器和 repository 路由中保持一致。
- 明确初始化数据、周期拉取数据、WebSocket 实时数据、缺口修复数据分别是否广播、是否落库，避免漏发或重复处理。
- 确认服务端和客户端对时间区间的开闭语义一致，边界点不会遗漏或重复。
- 检查新增表和服务在所有相关生产初始化入口都已接线，不因只更新一侧初始化而判定完成。

### 2. 测试与命令

- 检查针对性测试是否覆盖 VO→服务端 PO→protobuf→客户端 PO 的字段转换、nullable、时间精度和 ID 映射。
- 检查多种上游形态、payload variant、缺失 instrument、历史区间边界、冲突写入和 SQL/schema 一致性等适用边界。
- 运行与变更范围匹配的最小检查，例如：
  - `cargo fmt --check`
  - `cargo check -p yu --examples`
  - `cargo test -p yu <目标测试>`
  - 涉及 `yue` 或 `li` 时，追加对应 crate 的最小 `cargo check`/目标测试。
- 只记录实际执行的命令和结果。命令未运行、失败或因环境受阻时，明确说明；不得声称未执行的检查已通过。

### 3. 结果报告

- 按严重程度列出确定缺陷，每项包含文件及行号、预期行为、实际行为和影响；将业务语义不明确的问题单独标为待确认。
- 主要按照上面几个Skill是创建的内容，是否有缺失，组装是否合适。
- 未发现问题时，明确写出检查了上述四类中的哪些范围及实际通过的检查；未检查的端或功能不能声称通过。
- 若问题依赖协议、schema、业务唯一键、历史/实时行为或同步范围的选择，先向用户确认，不擅自决定。
- 默认仅报告，不修复。用户明确要求修复时，先按严重程度和影响范围确认问题，再做最小修复并重跑相关检查。

## 判定原则

- 以生产调用链为准，沿现有范例核对；不要因同名方法、trait、文件或测试存在而判断完成。
- 优先复用现有 repository、转换、调度和错误处理模式；不增加与本次目标无关的 CRUD、抽象、任务或数据类型。
- 保留 `Option` 的缺失语义，不以零值、空字符串或静默忽略掩盖缺失数据。
- 数据库错误应传播，不吞错、不伪造成功；批次写入和唯一约束冲突按现有模式可靠处理。
- 按 Cargo 分层遵循 `yu -> yue -> li`，允许 `yu -> li`，禁止反向依赖。
- 金融数值优先使用 `rust_decimal::Decimal`；时间单位、精度和时区沿完整链路核对。
- 检查或修复时，不修改无关文件、用户注释或既有说明，不覆盖工作区中的其他改动。
