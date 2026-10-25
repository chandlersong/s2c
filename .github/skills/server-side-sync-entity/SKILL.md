---
name: server-side-sync-entity
description: 同步业务对象时，服务器端需要做的工作。主要是History和Instrument的service和repository的实现，以及接线和示例。客户端的处理不在本技能范围内。
---

# 根据 PO 或 VO 完成同步链路

## 目标

用户提供指定业务对象后，比如指定PO，VO后，沿现有同步示例识别该数据需要经过的组件repository、service、协议和启动接线，使数据能够按项目现有机制从服务端同步到客户端。已有实现先复用，不因输入是
VO 就默认重建 instrument 链路。

## 参考范例

- 组件参考: 在`yu/examples/sync_server_polymarket_token_example.rs`中，其列出了在服务器端，同步一个对象所需要的组件以及需要实现的方法。
- 组装组件参考：如何组装请参考`yu/src/polymarket/sync_job.rs`中的start_polymarket_sync_series_job方法
- 程序入口:`yu/src/bin/yu_sync_server.rs`

优先复用示例采用的业务实现和既有类型；示例未覆盖的客户端链路，继续追踪客户端 `sync_client`、repository和service
的相邻实现。只检查与本次数据链路相关的代码。

## 实施流程

### 1. 补齐服务器端的内容

以下每一个小节都是需要补全的内容。依据所要求的内容，参考现有的示例和实现，补全缺失的组件和逻辑。

#### History相关

History相关的内容。包括repository、service

###### service

具体可以参考：`yu/src/polymarket/sync_server.rs`的`PolyMarketSyncHistoryService`中的方法。

必须实现的功能：

- 接收一个`history_tx: Sender<HistoryEntry>`，并在`broadcast_message`中发送消息。
    - 具体可以参考`PolyMarketSyncHistoryService`中的`broadcast_message`方法。
    - 在该方法中，需要把PO转换成HistoryEntry然后发送。
- `for_production`方法，用于在生产函数调用
    - 具体参数可以参考`PolyMarketSyncHistoryService`中的`for_production`方法。
    - 如果业务需要，可以自行添加参数
- 实现`SyncHistoryServiceTrait`这个trait，具体可以参考`PolyMarketSyncHistoryService`中的实现。

可以选择部分实现的功能：
需要实现这些功能。但是如果无法确定。则可以询问用户是否需要实现这些功能或者留空。不能擅自生成。

- `initial_history_data`方法：初始化历史数据。
    - 默认使用restful API获取历史数据，具体可以在yu和yue包里面查看有没有相应的API，如果没有，就询问用户是否需要自己实现获取历史数据的逻辑。不允许擅自生成
    - 这里的数据直接进数据库。这里的数据不需要发送到客户端。
    - 有些如果没有办法活的历史数据的，比如`OkxOptionSummaryPo`，则可以不实现这个方法,如需打印log，打印debug级别的。但是需要写上注释。
- `sync_all`方法：获取最新的数据。
    - 具体可以在yu和yue包里面查看有没有相应的API，如果没有，就询问用户是否需要自己实现获取历史数据的逻辑。不允许擅自生成
    - `PolyMarketSyncHistoryService`为restful的实现。但是实际可能有其他实现。
    - sync_all方法获取的数据需要发送到客户端。
    - sync_all活的的两种方式
        - restful API获取最新数据：具体可以参考`PolyMarketSyncHistoryService`中的`sync_all`方法。
            - 需要在组装的地方，周期性调用。因为现在基本都是1小时为周期，原则上，应该随机在整点的90秒内调用。
        - websocket获取最新数据，具体可以参考`OkxSyncHistoryService`中方式。
            - 不用在组装的地方，加入周期性调用。
- `check_history_data`方法：检查本地数据。
    - 模仿`PolyMarketSyncHistoryService`中用现有的二分法查找工具来实现这个方法。
    - 补全数据，模仿自己的initial_history_data方法来实现这个方法。
    - 检查出的缺失数据，在补全后，要发给客户端。

###### repository

- history的repository：`yu/src/polymarket/duckdb_repository.rs`的`PolyMarketHistoryRepositoryTrait`
- 根据service的业务需求，生成相应的办法。

#### Instrument相关

Instrument相关的内容。包括repository、service。
但是非必要选项。有时候可以忽略跳过。

###### service

具体可以参考：`yu/src/polymarket/sync_server.rs`的`PolyMarketSyncInstrumentService`中的方法。

必须实现的功能：

- `list_all_instruments`方法：列出所有的instrument。包括所有的instrument的PO。
    - 具体可以参考`PolyMarketSyncInstrumentService`中的`list_all_instruments`方法。
- `for_production`方法，用于在生产函数调用
    - 具体参数可以参考`PolyMarketSyncInstrumentService`中的`for_production`方法。
    - 如果业务需要，可以自行添加参数
- 维护一个`live_instruments`的缓存
    - 需要能在多个环境中共享，方式方法包括但是不限于`PolyMarketSyncInstrumentService`中的加锁的方法。
- `support_history_list`方法：返回支持的历史数据类型列表。
    - 具体可以参考`PolyMarketSyncInstrumentService`中的`support_history_list`方法。
    - 优先使用外部注入的方式。

可以选择部分实现的功能：
需要实现这些功能。但是如果无法确定。则可以询问用户是否需要实现这些功能或者留空。不能擅自生成。

- `sync_instruments`方法：同步instrument。
    - 具体可以参考`PolyMarketSyncInstrumentService`中的`sync_instruments`方法。
    - 需要维护上面提到的`live_instruments`缓存。

注意事项：

- 应直接依赖于各个repository的接口，而不应该依赖于其他service的接口。

###### repository

- history的repository：`yu/src/polymarket/duckdb_repository.rs`的`PolyMarketInstrumentRepositoryTrait`
- 根据service的业务需求，生成相应的办法。

### 2. 接线、示例与验证

#### 组装代码

组装氛围两部分。

- 模仿`yu/src/polymarket/sync_job.rs`中的`start_polymarket_sync_series_job`方法，组装自己的同步链路。
    - 关于HistoryService的组装。必须做以下几件事
        - 调用initial_history_data方法，初始化历史数据。且其调用时间，应该在InstrumentService初始时sync_instruments方法之后。
        - 每隔6小时调用一次check_history_data方法，检查本地数据。具体分钟数随机。但不能是整点活着5的倍数。
        - 每隔1小时调用一次sync_all方法，获取最新的数据。
    - 关于InstrumentService的组装。必须做以下几件事
        - 调用list_all_instruments方法，获取所有的instrument。
        - 必须在启动时调用一次sync_instruments方法，获取最新的instrument。
        - 每隔6小时调用一次sync_instruments方法，获取最新的instrument。具体分钟和秒是随机。但不能是整点活着5的倍数。
- 在`yu/src/bin/yu_sync_server.rs`中，调用上面部分的代码。然后注册到`YuSyncServer`中
    - 需要检查配置文件是否启用改服务。

#### 示例代码

- 创建自己的example，命名规则为 `yu/examples/sync_server_{history_type}_example.rs`，例如：
  `yu/examples/sync_server_polymarket_token_example.rs`
    - 不允许直接使用sync_job.rs，必须显示的调用这些方法。
    - 必须显示的调用一遍sync_job.rs的方法

#### 验证

- 运行最小相关格式化、编译和测试命令（例如 `cargo fmt`、`cargo check -p yu --examples`、目标模块测试）；仅报告实际运行的检查。

## 决策与约束

- 用户提供 PO/VO 不代表授权或要求更新 instrument；是否新增 instrument 由目标链路缺口决定。若 history 只能在新增 instrument
  后才能正确映射，而用户意图不明确，先询问。
- 只有在代码无法判定且会改变行为/范围时才向用户确认，例如是否需要实时广播、全量初始化回补、完整性修复，或 instrument
  是否允许新增。可从示例和现有生产接线确认的事项不要重复询问。
- 严格核对时间单位和精度：沿 API、VO、PO、proto、SQL、追踪转换；不得把秒、毫秒和数据库时间类型混用。保留 `Option`
  语义，不以默认值掩盖缺失数据。
- 写入需幂等；唯一约束冲突应按现有 repository 模式处理，不可静默丢弃有效数据。
- 遵循 Cargo 分层和现有错误处理；不复制查询逻辑、不增加无关抽象，不改无关文件、用户注释或既有说明。
- 尊重工作区未提交改动；不要读取、输出或修改凭证及私有环境配置。
- 只是做服务器端的处理，而不涉及客户端的任何处理。
- 如果一些方法业务上不需要实现，或者无法实现，则可以不实现。并在代码中写上注释说明原因。
    - 注释级别为debug级别，且必须写上原因。