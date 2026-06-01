# S2C AI Agent 执行指南

## 1) 架构边界（必须按 Cargo 分层）
- `li/`（基础设施层）：通知、RocksDB、AWS、Actix 定时任务与 WebSocket 客户端能力（见 `li/src/lib.rs`）。
- `yue/`（交易所层）：币安 REST/WebSocket、模型转换、HTTP 客户端（见 `yue/src/lib.rs`）。
- `yu/`（应用层）：配置、调度、数据完整性、DuckDB、Arrow Flight 服务（见 `yu/src/lib.rs`）。
- 依赖方向固定：`yu -> yue -> li`，且 `yu -> li` 允许；禁止反向依赖（见各 crate `Cargo.toml`）。

## 2) 关键数据流（以 `yu_datacenter` 为主）
- 入口：`yu/src/bin/yu_datacenter.rs`。
- 启动顺序：读取配置 `get_config()` -> 初始化日志 `setup_logger()` -> 初始化 HTTP 客户端代理 `init_http_client()` -> `start_bn_jobs()`。
- `start_bn_jobs()`（`yu/src/binance/jobs.rs`）会：
  1. 初始化/刷新 Binance Dashboard，并注册 `0 01 * * * *` 定时刷新；
  2. 创建 DuckDB 表 `initial_tables(None)`；
  3. 启动 Spot/Swap Kline WebSocket，并执行历史 Kline 回补 `initial_kline(...)`；
  4. 同步 Funding Rate：`start_sync_funding_rate(...)`；
  5. 启动数据完整性定时修复任务：`start_data_integrity_jobs(...)`。
- 默认入口当前**不启动** `start_monitor_account()` 和 `start_spot_websocket_stream_job()`；这两条链路保留在 `yu/src/binance/jobs.rs` 中按需接线。
- 服务输出：启动 Arrow Flight 服务 `0.0.0.0:8815`（`yu_datacenter.rs`），并注册 `0 08 * * * *` 数据清理任务；收到 Ctrl+C 后通过 `System::current().stop()` 优雅退出。
- `yu_mcp` 入口：`yu/src/bin/yu_mcp.rs`，启动顺序为 `get_config()` -> `setup_logger()` -> `init_http_client()` -> `SseServer::serve("127.0.0.1:8000")`；当前直接挂载 `yu/src/binance/bn_mcp.rs` 的 `BinanceSpot::new`，默认 MCP tool 只有 `price_change_24h`。

## 3) 构建 / 测试 / 运行命令
- 全量测试（CI 同款）：`cargo test --all-features`（见 `.github/workflows/dev.yml`）。
- 工作区编译：`cargo build --workspace`。
- 运行数据中心：`cargo run -p yu --bin yu_datacenter`。
- 运行 MCP 服务：`cargo run -p yu --bin yu_mcp`（默认 `127.0.0.1:8000`）。
- 运行示例：
  - `cargo run -p yu --example duckdb_example`
  - `cargo run -p yu --example data_integrity_example`
  - `cargo run -p yu --example bn_spot_stream_example`
  - `cargo run -p yue --example bn_restful_examples`
  - `cargo run -p yue --example order_book_example`
- CI 容器调试参考：`make test_build_in_docker`（进入 `chandlersong/rust_ci:1.89-slim-bookworm` 容器 shell；当前与 `.github/workflows/dev.yml` 使用的 `1.93-slim-bookworm` 不一致）。

## 4) 项目特有约定（可直接检查）
- 配置文件：默认 `config.toml`，可用 `CONFIG_PATH` 覆盖（`yu/src/config.rs`）。
- 环境变量覆盖：支持 `YU_` 前缀，且用下划线映射嵌套键（如 `YU_PROXYURL`、`YU_DATABASE_PATH`，见 `config::Environment::with_prefix("YU").separator("_")`）。
- DuckDB：`database.path` 未配置时退回内存库；仓库内现成样例见 `local_config/yu_datacenter.yaml`，默认落库示例路径是 `testdata/mingyu/mcp.db`（`yu/src/duck_db.rs`）。
- 数据完整性默认值：`periodic_check_interval_cron = "0 6,36 * * * * *"`，`repair_backoff.max_retries = 3`（`yu/src/config.rs`）。
- 交易对必须大写（如 `BTCUSDT`，见 `.github/copilot-instructions.md` 与 `local_config/yu_datacenter.yaml`）。
- 金融数值优先 `rust_decimal`，避免 `f64`（workspace 依赖统一声明）。

## 5) 跨组件集成方式（推荐复用）
- 市场数据 WebSocket 核心抽象为 `li/src/websocket/connection.rs`：`WebSocketConnection` / `WebSocketInterface` / `MessageHandlerTrait`（Tokio 模式）；`yu/src/binance/websocket_service.rs` 的 `KlineSubscribeService` 仅为 yu 侧示例编排。
- 业务处理优先实现 `MessageHandlerTrait`（如 `SpotKlineSaver`、`SwapKlineSaver`）；`WebSocketConnection` 在读取 `WsMessage::Text/Binary` 后分别调用 `M::from_text` / `M::from_binary`，再交给 handler。
- 连接控制统一通过 `WebSocketInterface::command_sender()` 发送 `CommandMessage`：常用 `CommandMessage::Connection(ConnectionAction::Close)`（符号变更时先建新连再关旧连）。
- `WebSocketEvent` 仅用于连接态广播：`Connected` / `Disconnected` / `Reconnecting` / `Error`；不承载逐条行情消息。
- `start_spot_websocket_stream_job()` + `li/src/websocket/client.rs` 属于旧的 Actix 订阅式链路（`client.rs` 已 `#[deprecated]`），仅兼容参考，不作为新默认模板。
- 账户监听链路参考：`yu/src/binance/jobs.rs` 中 `start_monitor_account()`；普通账户走 `SpotAccountActor` + `ListenKeyClient::swap(...)`，统一账户走 `ListenKeyClient::portfolio(...)`。
- 新增交易所接入时，优先在 `yue/` 做协议适配，在 `yu/` 做调度与落库，`li/` 仅放通用能力。
