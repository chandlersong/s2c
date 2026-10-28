---
name: create-sync-model-by-vo
description: 根据指定 VO 创建服务端/客户端 PO、protobuf 实体、对应表定义及转换测试，不接入运行时服务。
---

# 根据 VO 创建业务实体

## 目标与范围

根据用户指定的 VO，创建或补齐该业务对象在同步数据模型中的实体定义和字段转换。只允许做以下与目标实体直接相关的改动：

- 服务端 PO 及其 VO → PO 转换，放在该业务对象所属的模型文件中。
- `yu/proto/history_data.proto` 中对应的 protobuf message；仅在承载该实体所必需时，追加对应的 enum 值或 `HistoryEntry.oneof` 字段。
- `yu/src/sync/models.rs` 中对应的 PO → protobuf 转换（遵循仓库既有放置模式）。
- 客户端 PO 及其 protobuf → 客户端 PO 转换，放在对应客户端 PO 文件中。
- 服务端和客户端该实体对应的表 DDL，以及让表被既有初始化入口识别所需的最小表注册改动。
- 仅验证本次实体字段映射和 schema 对齐所需的针对性测试。

只添加目标对象所需的字段和转换，不重写或重命名已有模型，不改动 VO 的定义或业务语义。若目标端已有等价实体，检查后复用或只补齐缺失字段，不重复创建。

## 明确禁止

除非用户在当前请求中明确要求，否则不得创建、修改或接线以下内容：

- repository、service、controller、RPC/gRPC service、RESTful、WebSocket、job、调度、采集或广播逻辑。
- 同步请求/订阅流程、运行入口、依赖、通用抽象或其他业务对象。
- 与本次 PO/protobuf/客户端 PO 转换或实体 schema 无关的测试、测试文件或测试函数。
- 数据写入器、查询逻辑、冲突处理流程或数据库初始化实现；只允许添加既有初始化入口需要的新表注册，不修改初始化流程本身。

`history_data.proto` 中的 message、enum 值和 `oneof` 字段属于实体传输定义，可以按上述范围添加；不得添加或修改 RPC service、请求流程或运行时分发逻辑。

## 开始前检查

1. 定位用户指定 VO 的定义、字段类型及已有 PO/protobuf/客户端 PO。
2. 只读检查与字段语义直接相关的转换代码，确认 nullable、金融数值类型、时间单位/时区、实体 ID 与关联 ID 的含义。
3. 不沿运行入口扩展调查服务端查询、订阅、消费或 repository；只定位新表所需的既有表注册列表，不扩展到数据库初始化流程。
4. 只有字段语义或 ID/时间映射存在无法从 VO 和相邻模型推断的歧义，且会改变实体字段或 protobuf 兼容性时，才使用 `ask_user` 澄清。

参考模式：

- VO：`yue/src/okx/models/restful.rs` 中的 `OptionSummaryDetail`。
- 服务端 PO：`yu/src/okx/duck_po.rs`。
- 服务端表：`yu/src/okx/duckdb_consts.rs` 中对应的实体 DDL。
- protobuf message：`yu/proto/history_data.proto` 中的 `OkxOptionSummary`。
- 客户端 PO：`yu/src/sync/client/po/okx.rs` 中的 `LocalOkxOptionSummaryPo`。
- 客户端表：`yu/src/sync/client/db_consts.rs` 中对应的实体 DDL。
- PO/protobuf 转换：参考相邻业务模型的既有实现位置和风格。

## 实施约定

- 先核对字段一一映射，再仅在上述模型文件和对应的表定义/注册位置实现缺失实体、转换与 schema。
- 表定义只反映目标实体字段和已确认的业务键；不创建 repository、writer、查询或冲突处理逻辑。
- `Option<T>` 保持可空；不得无依据转成零值、空字符串或其他默认值。
- 明确秒/毫秒/纳秒及时间时区，转换中保留原始精度。
- 金融数值优先使用仓库约定的 Decimal；若 VO、protobuf 或既有实体使用浮点类型，不擅自改变协议类型，并注意精度影响。
- 服务端 ID、客户端 PO 主键、客户端本地关联 ID 各自语义清晰，不混用。不得臆造业务键。
- protobuf 仅追加未使用的字段号和 enum 值；不得复用、删除或改号已有字段。
- DDL 列、客户端 PO 字段及批量序列化列（若已有转换实现使用该列序列）必须逐列一致；不得顺带修改既有表或其它实体。
- 不改动用户已有注释、说明文档及无关文件。

## 验证

- 添加针对性测试，验证 VO → 服务端 PO → protobuf → 客户端 PO 的字段、nullable、ID 和时间精度；若有不同上游 VO 形态，覆盖各形态。
- 必要时添加轻量 schema 对齐测试，验证 DDL/注册与 PO 字段对应；不测试或实现 repository、writer、service、RPC 或运行时同步链路。
- 仅对本次修改的 Rust 文件运行格式化，并运行最小相关编译和测试。
- 最终说明新增或补齐了哪些业务实体/转换，并明确任何因禁止范围而未处理的表或运行时接线；不得声称未执行的验证已通过。
