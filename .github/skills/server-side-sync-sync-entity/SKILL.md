---
name: server-side-sync-sync-entity
description: 同步业务对象时，服务器端需要做的工作。根据用户提供的 PO 或 VO 作为要同步的业务对象的实体，参照同步示例补齐缺失的服务端history 与 instrument 同步链路，并完成接线。
---

# 根据 PO 或 VO 完成同步链路

## 目标

用户提供指定业务对象后，比如指定PO，VO后，沿现有同步示例识别该数据需要经过的组件，补齐缺少的模型、持久化、repository、service、协议和启动接线，使数据能够按项目现有机制从服务端同步到客户端。已有实现先复用，不因输入是
VO 就默认重建 instrument 链路。

## 参考范例

- 组件参考: 在`yu/examples/sync_server_polymarket_token_example.rs`中，其列出了在服务器端，同步一个对象所需要的组件以及需要实现的方法。
- 组装组件参考：如何组装请参考`yu/src/polymarket/sync_job.rs`中的start_polymarket_sync_series_job方法
- 程序入口:`yu/src/bin/yu_sync_server.rs`

1. 检查是否有相应的VO,PO以及数据库的表，以及相应的repository，如果确实，则创建。
2. 判断是否需要创建 instrument service，如果需要则执行 instrument 同步，再列出 instrument。
    - 具体参考`yu/src/polymarket/sync_server.rs`中的`PolyMarketSyncInstrumentService`

3. 需要创建 History service。
    - 具体参考`yu/src/polymarket/sync_server.rs`中的`PolyMarketSyncHistoryService`

优先复用示例采用的业务实现和既有类型；示例未覆盖的客户端链路，继续追踪客户端 `sync_client`、PO、repository、表和 service
的相邻实现。只检查与本次数据链路相关的代码。

## 实施流程

### 1. 整理和规范需要

根据下面的列表，以及参考中的内容，确定需要修改的任务列表。等待用户确认。

- 补全必要的组件 检查是否有相应的VO,PO以及数据库的表，以及相应的repository，如果确实，则创建。
    - instrument的PO可以参考：`yu/src/polymarket/po.rs`的`PolyMarketInstrumentPo`
    - history的PO可以参考：`yu/src/polymarket/po.rs`的`PolyMarketHistoryPo`
    - instrument表的定义：`yu/src/polymarket/db_consts.rs`的`polymarket_instruments`
    - history表的定义：`yu/src/polymarket/db_consts.rs`的`polymarket_price_history`
    - instrument的repository：`yu/src/polymarket/duckdb_repository.rs`的`PolyMarketInstrumentRepositoryTrait`
    - history的repository：`yu/src/polymarket/duckdb_repository.rs`的`PolyMarketHistoryRepositoryTrait`
    - instrument的service：`yu/src/polymarket/sync_server.rs`的`PolyMarketSyncInstrumentService`
    - history的service：`yu/src/polymarket/sync_server.rs`的`PolyMarketSyncHistoryService`
- 数据来源:
    - 如果能够自主确定，请让用户确认是否正确。确认的内容为具体调用了哪些方法和方式
    - 如果不能确定，不能主动生成，生成一个空方法，加入todo。后续有用户来处理。
- 任务列表的格式，必须包含下面内容
    - 具体哪些类和文件
    - 实现哪些方法

### 2. 补齐服务端

根据上一份报表开始执行，按数据类型选择所需部分：

- **History**：相应的repository。如果有必要补全po和protobuf的内容。
- **History service**：沿用示例的 `SyncHistoryServiceTrait`
  模式，实现目标数据的查询、入库、广播及协议支持的历史区间查询。实时推送、初始化回补、完整性检查按现有业务需求接入；批量查询、channel
  关闭和错误传播遵循相邻实现。
- **Instrument**：若列表、协议映射、稳定 server ID 或客户端 instrument 映射已存在，则复用，不更新 instrument。只有本次明确要求同步
  instrument，
    - 提供一种方式，让其他访问到最新的还在工作的instrument。包括不限于在PolyMarketSyncInstrumentService，一个加锁的共享方式。
- **协议**：复用已有 protobuf 消息、oneof 分支和 `HistoryType`；确需新增时更新 proto 及生成代码所需的仓库流程，不复用已分配字段号。保证
  instrument key 使用稳定业务标识，history 消息使用协议约定的 server instrument ID。
    - 具体应用，组装请参考`yu/src/polymarket/sync_job.rs`中的start_polymarket_sync_series_job方法。然后应用到
      `yu/src/bin/yu_sync_server.rs`中

### 4. 接线、示例与验证

- 按模块约定导出新增模块；接入服务端启动注册和客户端类型/服务路由。
- 创建自己的example，命名规则为 `yu/examples/sync_server_{history_type}_example.rs`，例如：
  `yu/examples/sync_server_polymarket_token_example.rs`
- 添加适配现有测试模式的针对性测试，重点验证 VO→PO、PO→proto、oneof/type、server/local ID 映射、history 落库和错误传播。
- 运行最小相关格式化、编译和测试命令（例如 `cargo fmt`、`cargo check -p yu --examples`、目标模块测试）；仅报告实际运行的检查。

## 决策与约束

- 用户提供 PO/VO 不代表授权或要求更新 instrument；是否新增 instrument 由目标链路缺口决定。若 history 只能在新增 instrument
  后才能正确映射，而用户意图不明确，先询问。
- 只有在代码无法判定且会改变行为/范围时才向用户确认，例如是否需要实时广播、全量初始化回补、完整性修复，或 instrument
  是否允许新增。可从示例和现有生产接线确认的事项不要重复询问。
- 严格核对时间单位和精度：沿 API、VO、PO、proto、SQL、客户端 PO 追踪转换；不得把秒、毫秒和数据库时间类型混用。保留 `Option`
  语义，不以默认值掩盖缺失数据。
- history 的数据库 `instrument_id` 使用对应数据库中的 instrument 主键；跨端传输及本地映射使用 server ID，不可混淆或随机生成。
- 写入需幂等；唯一约束冲突应按现有 repository 模式处理，不可静默丢弃有效数据。
- 遵循 Cargo 分层和现有错误处理；不复制查询逻辑、不增加无关抽象，不改无关文件、用户注释或既有说明。
- 尊重工作区未提交改动；不要读取、输出或修改凭证及私有环境配置。
