# 账户更新数据入库设计（Spot，yue 模块）

- 范围与约束
  - 仅设计 Spot 阶段的账户数据同步与入库。
  - 组件命名采用 AccountSync，不做账户计算；管理器命名为 AccountSyncManager，归属 `yue` 模块。
  - 订阅来源：Binance `userDataStream.subscribe.signature`（支持多账户）。
  - 存储统一使用 `yu` 中的 DuckDB 能力（复用，不修改现有代码）。
  - 交易对统一使用大写（例如 BTCUSDT）。
  - 考虑扩展到 Future/Swap/Option，但本阶段不实现。

## 1. 流程图

```
[加载账户配置]
        |
        v
[初始化 AccountSyncManager (yue)]
        |
        +--> [为每个账户创建 AccountSyncWorker]
                    |
                    v
             [获取/刷新 listenKey]
                    |
                    v
          [订阅 userDataStream.signature]
                    |
                    v
          [接收事件: 余额/订单/账户更新]
                    |
                    v
          [事件标准化(Spot): 统一模型+大写symbol]
                    |
                    v
          [幂等检查与去重]
                    |
                    v
          [批量写入 DuckDB (复用 yu 能力)]
                    |
                    v
          [心跳与重连: 监听过期/错误隔离]
        |
        v
[监控与审计: 指标/日志/raw 回放]
```

说明：
- 多账户并行，每个账户维护独立 listenKey 生命周期与错误域；订阅断开不影响其他账户。
- 事件在 yue 中标准化为统一 Spot 模型，后续可通过不同市场模型扩展。
- 入库通过 yu 的 DuckDB 接口，控制事务与批量，提升吞吐与一致性。

## 2. Cargo 与模块对应

- yue
  - 新增（设计层面，不改码）：`account_sync`
    - `AccountSyncManager`：读取配置，创建/管理多账户 worker，健康监控与生命周期控制。
    - `AccountSyncWorker`：单账户订阅、事件消费、标准化、幂等与入库调用。
    - `models::spot`：Spot 统一事件模型（余额、订单、账户更新等）。
    - `pipeline`：事件处理流水线（校验→标准化→幂等→批量入库）。
    - `ext::binance`：适配 `userDataStream.subscribe.signature`，封装 listenKey 管理与重订阅。
- yu
  - 复用：`duck_db.rs` 存储能力，作为统一的数据库写入层。

## 3. 事件模型（Spot）

统一字段建议（示意）：
- 通用元数据：`exchange=BINANCE`, `account_id`, `event_time`, `event_type`, `symbol(大写)`, `raw_json`
- 余额更新：`asset`, `free`, `locked`
- 订单事件：`order_id`, `client_order_id`, `status`, `side`, `type`, `price`, `qty`, `exec_qty`, `last_exec_price`

幂等键建议：
- 订单事件：`account_id + order_id + event_time`
- 余额事件：`account_id + asset + event_time`

## 4. DuckDB 表设计（Spot）

- `account_balance_spot`
  - 字段：
    - `account_id TEXT`
    - `asset TEXT`
    - `free DOUBLE`
    - `locked DOUBLE`
    - `event_time TIMESTAMP`
    - `source_exchange TEXT`  -- 固定为 "BINANCE"
    - `raw_json JSON`
  - 索引与主键建议：
    - 复合唯一键 `(account_id, asset, event_time)`

- `order_events_spot`
  - 字段：
    - `account_id TEXT`
    - `symbol TEXT`            -- 大写，例如 BTCUSDT
    - `order_id TEXT`
    - `client_order_id TEXT`
    - `status TEXT`
    - `side TEXT`
    - `type TEXT`
    - `price DOUBLE`
    - `qty DOUBLE`
    - `exec_qty DOUBLE`
    - `last_exec_price DOUBLE`
    - `event_time TIMESTAMP`
    - `source_exchange TEXT`   -- 固定为 "BINANCE"
    - `raw_json JSON`
  - 索引与主键建议：
    - 复合唯一键 `(account_id, order_id, event_time)`
    - 业务查询索引 `symbol, event_time`


说明：
- 保留 `raw_json` 以便审计与回放。
- 使用时间戳作为幂等和查询维度，杜绝重复入库。
- DuckDB 可通过批量插入与事务控制实现高吞吐与一致性。

## 5. 多账户订阅与配置

- 账户配置项（配置文件）：
  - `account_name`
  - `api_key`
  - `secret_key`
- 运行参数（不写入配置文件，通过启动参数/环境变量/全局配置覆盖）：
  - `batch_size`（默认100）
  - `flush_interval_ms`（默认1000）
  - `reconnect_interval_secs`（默认5）
  - `max_reconnect_attempts`（默认10）
- 行为：
  - `AccountSyncManager` 读取配置，按账户启动独立 `AccountSyncWorker`。
  - 每个 worker 独立维护 listenKey，定时刷新与过期重订阅，错误隔离。
  - 写入可采用 per-account 队列，批量/合并入库，控制事务与锁。

## 6. 事件处理与幂等策略

- 幂等：基于复合键在 DuckDB 做去重；可选引入轻量 kv 记录最近水位。
- 断点：userDataStream 不提供历史，断流后从上次成功入库时间对齐，允许少量数据丢失并告警。
- 重试：网络错误重试（指数退避），持久化失败记录并告警。

## 7. 扩展性预留（Future/Swap/Option）

- 抽象：`AccountSyncWorker<TMarket>` 泛型化，当前实现 `TMarket=Spot`。
- 模型分层：`models::spot`, `models::swap`, `models::future`, `models::option`。
- 适配层：`ext::binance::user_stream_spot`/`usdtm`/`coinm`/`option`。
- 存储：不同市场有独立表，复用通用字段，扩展专有字段（杠杆、资金费、希腊值）。
- 管理：`AccountSyncManager` 面向多市场 worker 队列，按配置开关初始化。
- 实施策略：
  - 当前（Spot）：完整实现所有组件。
  - 后续（Swap/Future/Option）：复用现有架构，新增对应市场的模型、表和适配层，无需修改核心逻辑。

## 8. 非功能需求与权衡

- 可扩展性：
  - 优点：事件模型分层、管道化、DuckDB 表分品类；利于扩展。
  - 风险：不同市场事件差异大，统一成本高。
- 可靠性：
  - 优点：多账户错误隔离、listenKey 刷新与重连、心跳监控。
  - 风险：断流不补历史，需要容错与监控。
- 性能：
  - 优点：DuckDB 批量写入与列存适合事件落地；并发 worker 提升吞吐。
  - 风险：并发插入需控制锁与事务；建议批量与合并策略。
- 安全：
  - API Key 加密存储与最小权限订阅；配置访问控制。
- 运维：
  - 指标：订阅数、心跳、重连次数、入库延迟、失败率。
  - 日志：关键事件与 `raw_json` 审计；异常与丢失报警。

## 9. 实施步骤（不改现有代码）

- 设计：在 yue 中明确 `account_sync` 的结构与职责，定义 Spot 事件模型与表结构。
- 适配：复用 yue 的 Binance 客户端能力，封装 listenKey 管理与订阅接口。
- 管道：搭建标准化→幂等→批量入库流水线，复用 yu 的 DuckDB 写入。
- 多账户：配置加载、独立 worker 启动、健康监控与重连策略。
- 验证：使用测试账户进行端到端订阅与入库演练，压测并评估并发与吞吐。

## 10. 需求覆盖

- 使用 `userDataStream.subscribe.signature` 订阅：覆盖。
- 支持多个账户订阅并配置：覆盖。
- 存入数据库（DuckDB，复用 yu）：覆盖。
- 仅 Spot 阶段：覆盖（其他市场仅预留）。
- 命名与职责：AccountSync/AccountSyncManager（yue 内），不做账户计算：覆盖。

