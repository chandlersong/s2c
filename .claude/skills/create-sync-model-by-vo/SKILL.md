---
name: create-sync-model-by-vo
description: 根据用户指定的 VO，在 s2c 中创建完整的服务端/客户端同步模型，包括双方 PO、protobuf、双方表和 repository、转换映射及同步落库接线。
---

# 根据指定 VO 创建同步模型

## 目标

用户指定 VO 后，以它为业务语义来源创建一套可端到端同步和持久化的数据模型。不能只生成某一端的 PO 或 protobuf。完整工作包括：

- VO → 服务端 PO → protobuf → 客户端 PO 的字段转换，明确处理空值、时间精度和 ID 映射。
- 服务端和客户端各自的表定义及初始化注册；若表已存在则检查兼容并复用，不重复创建。
- 服务端和客户端各自常规 repository 能力及其调用方接线。
- 按任务范围接入历史同步、实时订阅或二者，并保证对应转换完整一致。

参考模式：`yue/src/okx/models/restful.rs` 中的 `OptionSummaryDetail` 是 VO。服务端 PO、DuckDB 表和 repository 参考 `yu/src/okx/duck_po.rs`、`yu/src/okx/duckdb_consts.rs`、`yu/src/okx/duckdb_tables.rs`、`yu/src/okx/duckdb_repository.rs` 中对应 Option Summary 的实现。客户端 protobuf、客户端 PO、PostgreSQL 表和 repository 参考已有同步模型中最接近的实现。

## 开始前确认

先定位用户指定 VO 的定义和生产点，再沿完整链路检查服务端 PO/表/repository、protobuf、客户端 PO/表/repository、同步服务和消费入口。以 `OptionSummaryDetail` 模式为主要参考，复用现有结构和约定。

只有目标客户端数据库、业务唯一键或历史/实时行为无法从用户请求及邻近代码推断，且会实质改变协议/API/schema 时，才用 `ask_user` 澄清。不得因为用户只提供 VO 就遗漏另一端对象或表；不得臆造业务键、默认值或同步语义。

## 架构与发现

- 遵循 Cargo 依赖方向 `yu -> yue -> li`，禁止反向依赖。服务端/客户端数据库编排、PO、repository 和 protobuf 接线通常位于 `yu/`。
- 从指定 VO 及其生产/消费调用点确定字段类型、Decimal/浮点策略、nullable、时间单位、主键/业务键、关联对象和生命周期。
- 分别检查服务端与客户端的 PO、schema、表注册/初始化、repository 和 writer，再检查 protobuf 与两端同步转换；不能只查一侧。
- repository 参考 `yu/src/okx/duckdb_repository.rs` 的 `OkxOptionSummaryRepositoryTrait`、实现、构造入口及对应查询/插入操作。客户端 repository 依照客户端本地存储模式实现对应 trait/实现/构造器，不能把服务端 repository 当作客户端 repository。
- 区分业务 VO、网络 protobuf、服务端 PO 和客户端 PO，按数据流显式转换，不默认让不同职责的模型直接复用。
- 确认任务涉及历史记录、实时事件还是二者。若相邻同步类型已实现两条路径，按请求接入对应路径；不擅自增加采集来源、定时任务或完整性修复。
- 用户指定的数据库、表名、文件位置和同步范围优先；影响协议/API/schema 的重要选项不明确时先询问。

## 实施流程

如某项已完成，检查正确性后继续，不要重复创建。

1. **理解指定 VO 和数据流。** 明确字段语义、nullable、精度/时间单位、业务唯一键、服务端数据来源和客户端关联方式。
2. **创建/补齐服务端 PO。** 实现 VO → 服务端 PO 转换及数据库读写所需的 row/appender 参数转换；明确服务端业务 ID 与数据库本地 ID。
3. **创建/补齐服务端表和 repository。** 检查 DDL、表枚举/注册和初始化入口；按同步实际需要实现常规 repository 操作，例如插入/批量插入、范围查询、最新时间查询。沿用 Option Summary repository 的 trait、实现和构造入口模式，不无差别生成 CRUD。
4. **创建 protobuf 定义。** 定义传输消息并接入需要的 `oneof`、`ServerMessage`、枚举或请求。遵守 protobuf 兼容性：不可复用已占用字段号，不随意删除或改号已发布字段。
5. **创建/补齐客户端 PO 和映射。** 实现 protobuf → 客户端 PO 转换；明确远端 ID 如何关联客户端本地 instrument ID 和本地 PO 主键。
6. **创建/补齐客户端表和 repository。** 更新 PostgreSQL DDL、表枚举/注册、`ALL_CLIENT_TABLES` 或对应初始化入口；按客户端 repository 既有模式实现同步所需操作，并接入 batch writer/consumer。
7. **接通端到端同步。** 服务端 repository 查询结果映射为 protobuf，客户端消息映射为 PO 并持久化。按用户请求或邻近数据类型的现有模式接入历史同步和/或实时订阅。
8. **添加针对性测试并验证。** 覆盖 VO→服务端 PO→protobuf→客户端 PO 的字段、nullable、ID、时间精度，以及关键 repository/schema/writer 行为。运行最小相关格式化、编译和测试；不声称未执行的验证已通过。

## 持久化与同步约定

- 数据库错误沿用对应 crate 错误类型并传播；不吞错、不伪造成功、不把查询失败转为空结果。
- SQL 值使用绑定参数；表名等 SQL 标识符必须来自静态受控定义。
- 冲突目标基于明确业务唯一键，随机 ID 不能替代业务键。服务端 ID、客户端本地 ID 和关联外键不可混用。
- 范围查询声明开闭区间并稳定排序；批量 writer 沿用仓库的批次/背压机制，并保证列顺序、NULL 编码、flush 和失败处理符合现有语义。
- `Option<T>` 保持 nullable，不得无依据转成零值、空字符串或其他默认值。
- 确认秒/毫秒/纳秒和时区；protobuf、PO 与 SQL 列之间保留规定时间精度，尤其是 `TIMESTAMPTZ`。
- 金融数值优先使用仓库约定的 Decimal；仅当 protobuf/schema/存储路径要求时转换为浮点，并确认精度损失。

## 不在默认范围内

- 不修改 VO、采集规则或交易行为，除非同步转换确实需要且用户同意。
- 不新增通用 ORM、迁移框架、依赖、代码生成器或新的数据库抽象。
- 不扩展无关 CRUD、缓存、定时任务、完整性修复或其他数据类型；repository 只覆盖同步实际需要的常规操作。
- 不擅自改变 protobuf 已有字段语义、数据库约束、保留策略或实时同步默认启停配置。
- 不修改无关文件、用户注释或说明文档。
