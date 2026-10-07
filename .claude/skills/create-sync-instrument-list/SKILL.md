---
name: create-sync-instrument-list
description: 根据指定的服务端 PO，在 yu 中实现 SyncInstrumentServiceTrait 的资产列表服务，并注册到 YuSyncServer。
---

# 根据 PO 创建同步资产列表服务

## 目标

用户指定一个已存在的服务端 PO 后，创建对应的 `SyncInstrumentServiceTrait` 实现，并将实现注册到 `YuSyncServer` 的启动入口。沿用现有 protobuf `Instrument` 联合类型及已有业务服务，不重建完整同步模型。

## 开始前检查

- 阅读目标 PO 定义、其列表查询所在的 service/repository，以及该业务模块的 `mod.rs` 和同步相关代码。
- 参考 `yu/src/okx/duck_po.rs` 的 `InstrumentPo`、`yu/src/okx/sync_server.rs` 的 `OkxSyncInstrumentService`，以及 `yu/src/polymarket/sync_server.rs` 的 `PolyMarketSyncInstrumentService`。
- 阅读 `SyncInstrumentServiceTrait` 和 `YuSyncServer::list_instrument`，确认 trait 契约、错误处理和列表聚合行为；阅读 `yu/src/bin/yu_sync_server.rs` 确认真实启动及注册位置。
- 检查 `yu/src/sync/models.rs`、`yu/proto/history_data.proto`，确认 PO 对应的 proto 类型、`Instrument` oneof payload 分支和现有字段转换。复用已存在的转换，不重复定义或破坏字段语义。
- 尊重工作区已有的未提交改动；不要读取、输出或修改凭证、私有环境配置等无关文件。

## 实施步骤

1. **确认列表来源和映射。** 找到返回目标 PO 列表的既有异步 service API，确定 `HashMap<String, Instrument>` 的稳定业务键，以及 PO 到对应 proto 类型的转换和 payload variant。键必须使用该 PO 的外部业务标识，不使用随机数据库主键，除非现有协议明确如此。
2. **创建同步服务。** 在对应业务模块新增 `sync_server.rs`，定义专属 service struct 保存必要的既有业务 service 依赖。提供构造函数，返回 `SyncInstrumentService`（`Arc<dyn SyncInstrumentServiceTrait + Send + Sync>`）。
3. **实现 trait。** 在 `list_instruments` 中调用既有列表 API，将每条 PO 转换成对应 proto，再包装到 `Instrument.payload` 的正确 oneof 分支，并以稳定业务键插入 map。错误按相邻实现记录日志并转换/传播为 `YuError`，不得吞错或伪装为空列表。提供能说明业务来源的 `error_log`。
4. **接入模块及启动入口。** 按模块现有约定导出 `sync_server`。在 `yu/src/bin/yu_sync_server.rs` 中，从已启动的业务 service 创建同步服务，将其加入 `instrument_services`，并传给 `YuSyncServer::create_and_start`。不修改既有 `YuSyncServer` 聚合逻辑，除非核查确认其 trait 或注册机制不能承载该服务。
5. **验证完整调用链。** 核对目标 PO → proto 转换 → oneof variant → map key → `YuSyncServer` 注册；检查模块路径、错误类型和 trait 对象类型。添加针对性测试（若相邻代码已有相应测试模式），运行最小相关 `cargo` 格式化、编译或测试命令；只报告实际执行的验证。

## 约定与边界

- 遵循 Cargo 分层：业务数据及同步编排放在 `yu/`，不引入反向依赖。
- 优先复用业务 service、proto 转换和 `SyncInstrumentServiceTrait`；不复制数据查询逻辑，不增加新的数据库层。
- 严格保留 PO 和 proto 的字段语义、可空性、单位与精度；不擅自用默认值替代 `Option` 的语义。若既有 proto 无法表达目标 PO 字段，先检查协议设计与周边模式；涉及兼容性或行为选择时再向用户确认。
- 多个服务合并到同一个 map 时，检查 key 是否可能冲突；不得静默引入错误覆盖。存在真实冲突且无法从业务标识中消解时，先向用户确认 key 约定。
- 本 skill 的范围是为已有 PO 接入资产列表服务，不默认创建 PO、数据库表、完整历史数据同步链路或新的 protobuf 消息。
- 不修改无关代码、用户注释或现有说明文档，不改动无关未提交文件。
