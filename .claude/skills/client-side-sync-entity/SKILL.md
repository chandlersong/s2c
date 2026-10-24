---
name: client-side-sync-entity
description: 在 yu 中实现客户端 instrument 和 history 同步，包含本地落库与客户端接线。
---

# 完成客户端同步实体链路

## 参考范例

- 客户端 service：`yu/src/sync/client/service/polymarket.rs`
- 通用 gRPC 订阅与分发：`yu/src/sync/client/sync_client_service.rs`
- 客户端入口：`yu/src/bin/yu_sync_client.rs`
- Instrument 初始化示例：`yu/examples/sync_client_exchange_service_polymarket_example.rs`
- History 批量写入示例：`yu/examples/postgresql_batch_insert_example.rs`

## Instrument service

参考 `PolymarketClientInstrumentService` 实现：

- `for_production`：主要有下列功能
    - 构造服务端 instrument ID 到本地 instrument 的映射，并让在所有的 history service 中共享
    - 并创建支持的 history services，按 `HistoryType` 注册。
- `initial_data`：遍历已注册的 history services，对每个 service 调用 `initial_data(instrument.clone(), end_ms)`。
- `supported_history_services`：返回该 instrument service 管理的 history services，供 `SyncClientService` 注册。

## History service

参考 `PolymarketClientHistoryService` 实现：

- `for_production`：创建 repository、batch insert、gRPC manager 和 `BinarySearchDS`，设置 history interval，并接收服务端 ID
  到本地 instrument 的映射。
- `initial_data`：
    1. 按服务端 ID 查询本地 instrument；不存在时创建本地 PO，然后更新server_id的缓存。
    2. 查询本地最新 history 时间戳；无记录时从 instrument 的 `start_ms` 开始，有记录时从最新时间戳开始。
    3. 调用 `backfill_history`，同步到传入的 `end_ms`。
- `backfill_history`：用 `SyncRequest` 调用 `sync_history`，请求范围为 `(start_timestamp, end_timestamp]`；逐条校验
  `history_type`、payload variant 和服务端 instrument ID，再通过 batch insert 使用本地 instrument ID 落库。
- `reconcile_living_instrument_with_server`：
    1. 按 history interval 对齐校验的起止时间。
    2. 从 repository 获取仍在运行的本地 instruments。
    3. 对每个 instrument 调用 `binary_search_gap` 查找缺口。
    4. 对 `MissingData` 调用 `backfill_history` 回补缺失区间。
- `insert_history`：校验 history type、instrument type 和 payload variant；先从服务端 ID 映射缓存获取本地 instrument
  ID，缓存未命中时查 repository 并更新缓存；本地 instrument 不存在时记录日志并跳过，找到时调用 batch insert 落库。

## SyncClientService 接线

参考 `yu/src/sync/client/sync_client_service.rs`：

- `for_production`：创建各交易所 instrument service，将其 `supported_history_services` 合并到 history service 表，并按
  `InstrumentType` 注册 instrument service。
- `initial_data`：遍历服务端 instrument 列表，按 `instrument_type` 找到对应 service 并调用 `initial_data`。
- `start_subscribe`：调用 `subscribe_latest`，收到 `ServerMessage` 后按每条 entry 的 `history_type` 找到对应 history
  service，并调用 `insert_history`；订阅流结束后重连并重新订阅。
- `reconcile_living_instrument_with_server`：遍历已注册的 history services，逐个调用同名方法。

在 `yu/src/bin/yu_sync_client.rs` 中初始化客户端表、创建 `SyncClientService` 和 gRPC manager，启动订阅、获取服务端
instrument 列表并执行初始同步，再按现有调度方式启动 living instrument 对账。

## Repository、示例与验证

- 复用目标业务已有的 PO、表和 repository；缺少时再补齐。History 使用现有 batch insert，插入时传入本地 instrument ID，不能将服务端
  ID 当作本地 ID。
- 对目标业务新增或补齐对应的 instrument/history service；映射字段和时间单位按 PO、协议及 repository 实现核对。
- 需要独立验证时，按 `sync_client_exchange_service_polymarket_example.rs` 添加聚焦示例，展示连接、目标 instrument
  查找及初始化或对账。
- 运行与修改范围匹配的检查，例如 `cargo fmt --check`、`cargo check -p yu --examples` 和目标测试。
