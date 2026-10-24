---
name: client-side-sync-entity
description: '在 yu 中为指定业务对象实现客户端同步与本地落库，适用于新增或补齐客户端的 instrument、history、repository、service 和启动接线。'
---

# 完成客户端同步实体链路

## 目标

用户指定客户端需要同步的业务对象后，沿项目已有的 gRPC 客户端实现，补齐从服务端数据接收、业务对象映射、本地数据库写入，到启动接线和示例验证的完整链路。优先复用已有 PO、repository、service、表和协议；不要因为新增一个对象就重建已经存在的通用客户端同步能力。

本技能只处理客户端（主要在 `yu/`）的工作。除非用户明确要求，不修改服务端实现、协议定义或其他 crate 的职责边界。

## 参考范例

- 客户端主入口：`yu/src/bin/yu_sync_client.rs`
  - 初始化配置、日志和客户端表。
  - 创建 `SyncClientService` 与 gRPC 连接管理器。
  - 启动订阅，并异步从服务端获取 instrument 列表、初始化客户端数据。
  - 定期调用 `reconcile_living_instrument_with_server` 对账。
- 单个 instrument 的客户端 service：`yu/examples/sync_client_exchange_service_polymarket_example.rs`
  - 请求服务端 instrument 列表。
  - 按协议 payload 类型和服务端 ID 找到目标对象。
  - 调用客户端 exchange service 的 `initial_data` 和 `reconcile_living_instrument_with_server`。
- History 批量落库：`yu/examples/postgresql_batch_insert_example.rs`
  - 初始化客户端表并使用 `PostgresqlBatchInsertImpl<LocalPolyMarketHistoryPo>` 写入。
  - 示例展示 `HistoryEntry`、history payload、服务端 instrument ID 与本地 instrument ID 的关联，以及写入后的查询验证。

先检查上述参考实现及目标对象相邻的客户端代码，再确定缺失组件。示例中的 Polymarket 类型仅作模式参考，不要把它们硬编码进其他业务对象的实现。

## 实施流程

### 1. 梳理数据链路与既有实现

- 确认用户指定的是 instrument、history，还是两者；确定对应的协议 payload、HistoryType/InstrumentType、服务端 ID 和本地 ID。
- 从 gRPC 响应追踪到客户端 service、repository、PO、表结构和实际写入位置，核实每个字段的映射及时间单位。
- 搜索并复用已有的公共同步订阅、数据库初始化、repository 和批量写入实现；只新增目标对象确实缺少的部分。
- 若服务端协议缺少目标对象或 payload，先说明这是协议/服务端范围，不要在客户端伪造数据或擅自改写服务端。

### 2. 实现本地模型、repository 与映射

- 为目标数据复用或补齐本地 PO、表和 repository。表字段、SQL 类型、主键/唯一键及查询条件必须与 PO 和现有数据库实现一致。
- 将协议对象显式映射为本地 PO；保留 `Option` 的缺失语义，不用默认值掩盖缺失字段。
- History 写入应通过项目现有的批量插入机制（参考 `PostgresqlBatchInsertImpl`）或目标 repository 完成。确保每条数据使用正确的本地 instrument ID，并沿用现有幂等/冲突处理约定。
- 不要混淆服务端 instrument ID 与客户端本地 instrument ID。按现有 instrument 映射查询本地 ID；无法映射时按相邻实现的错误处理方式显式处理，不能静默写入错误关联。
- 严格核对 epoch 秒、毫秒及数据库时间类型。不要仅凭字段名推断单位，应追踪协议、转换和 SQL。
- 金融数值遵循仓库约定，避免在金额或价格计算中使用 `f64`；只有协议或现有接口明确要求时才在边界处转换，并保持转换范围最小。

### 3. 实现客户端 service

- instrument service 参考 `PolymarketClientExchangeService`：提供生产构造方法，并按现有模式实现 instrument 初始化和与服务端对账。
- 从服务端列表筛选对象时，同时校验 payload variant 和稳定的服务端 ID；不得只凭列表位置或未验证的可选字段匹配。
- history 接收和落库优先接入现有 `SyncClientService`/订阅机制及 repository，不要另建与公共同步客户端并行的 gRPC 消费循环。
- 服务构造、数据库访问和 gRPC 调用遵循现有 `Result`/`YuError` 错误传播方式。不能吞掉落库或同步错误并返回成功。
- 不要让一个 service 依赖另一个 service 的接口来绕过 repository；按现有客户端分层直接使用合适的 repository。

### 4. 启动接线

- 在 `yu/src/bin/yu_sync_client.rs` 中按现有入口接线：配置与日志、数据库表初始化、客户端服务构造、gRPC 连接、订阅、初始 instrument 数据及周期性对账。
- 通用 `SyncClientService` 已负责的能力应继续由它处理。新增业务 service 时只接入它需要的初始化或对账逻辑，不要重复注册订阅或重复初始化表。
- 当前示例入口约每 6 小时触发一次 living instrument 对账（cron 表达式 `0 28 */6 * * *`）。新增任务应与现有调度方式一致，并先确认该实体确实需要单独任务，避免重复对账。
- `initial_grpc_client_tables` 在入口和示例中的使用方式不同。接线前检查函数行为及调用上下文，保证表初始化顺序正确且避免无意义的重复初始化。
- 保持 Cargo 依赖方向 `yu -> yue -> li`（`yu -> li` 允许）；不要让基础设施层或交易所层反向依赖应用层。

### 5. 添加示例与验证

- 对需要独立调试的新业务 service，新增聚焦的 example，命名遵循相邻文件模式，例如 `yu/examples/sync_client_exchange_service_{entity}_example.rs`。
- 示例应展示真实客户端流程：读取配置、初始化日志和表、连接服务端、查找目标 instrument、执行初始化或对账，并在必要时检查本地查询结果。
- 对批量 history 写入，参考 `yu/examples/postgresql_batch_insert_example.rs`，验证 PO 字段、记录数、时间戳和 instrument ID 映射；不要仅验证插入函数被调用。
- 运行与修改范围匹配的格式化、编译和测试，例如 `cargo fmt --check`、`cargo check -p yu --examples` 和目标测试。只报告实际执行的命令和结果。

## 决策与约束

- 用户只指定 history 时，不擅自新增 instrument 同步；如果数据落库必须依赖 instrument 映射但其来源不明确，先询问。
- 用户只指定 instrument 时，不擅自补造历史回补或实时 history 订阅需求。
- 只有当现有代码无法判断且会改变行为或范围时才询问，例如是否要支持已下架 instrument、是否需要完整历史初始化或是否需要独立周期任务。能从入口和相邻实现确认的事项不要重复询问。
- 不将示例中的测试表名、固定 instrument ID、模拟数据、睡眠时间或日志级别复制到生产接线。
- 遵守当前工作区已有改动，不覆盖或回退用户的修改；只改本次客户端链路需要的文件，不修改用户注释和既有说明文档。
