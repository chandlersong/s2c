# S2C AI Agent 执行指南

## 1) 架构边界（必须按 Cargo 分层）
- `li/`（基础设施层）：通知、RocksDB、AWS、Actix 定时任务与 WebSocket 客户端能力（见 `li/src/lib.rs`）。
- `yue/`（交易所层）：币安 REST/WebSocket、模型转换、HTTP 客户端（见 `yue/src/lib.rs`）。
- `yu/`（应用层）：配置、调度、数据完整性、DuckDB、Arrow Flight 服务（见 `yu/src/lib.rs`）。
- 依赖方向固定：`yu -> yue -> li`，且 `yu -> li` 允许；禁止反向依赖（见各 crate `Cargo.toml`）。

## 2) 关键数据流（以 `yu_datacenter` 为主）
- 入口：`yu/src/bin/yu_datacenter.rs`。
- 启动顺序：读取配置 `get_config()` -> 初始化 HTTP 客户端代理 `init_http_client()` -> `start_bn_jobs()`。
- `start_bn_jobs()`（`yu/src/binance/jobs.rs`）会：
  1. 初始化/刷新 Binance Dashboard；
  2. 创建 DuckDB 表 `initial_tables()`；
  3. 启动 Kline WebSocket + 历史数据回补（REST）；
  4. 启动数据完整性定时修复任务。
- 服务输出：启动 Arrow Flight 服务 `0.0.0.0:8815`（`yu_datacenter.rs`）。

## 3) 构建 / 测试 / 运行命令
- 全量测试（CI 同款）：`cargo test --all-features`（见 `.github/workflows/dev.yml`）。
- 工作区编译：`cargo build --workspace`。
- 运行数据中心：`cargo run -p yu --bin yu_datacenter`。
- 运行 MCP 服务：`cargo run -p yu --bin yu_mcp`（默认 `127.0.0.1:8000`）。
- 运行示例：
  - `cargo run -p yue --example bn_restful_examples`
  - `cargo run -p yue --example order_book_example`
- Docker 本地构建参考：`make test_build_in_docker`（`Makefile`）。

## 4) 项目特有约定（可直接检查）
- 配置文件：默认 `config.toml`，可用 `CONFIG_PATH` 覆盖（`yu/src/config.rs`）。
- 环境变量覆盖：支持 `YU_` 前缀（如 `YU_PROXYURL`，见 `config::Environment::with_prefix("YU")`）。
- 交易对必须大写（如 `BTCUSDT`，见 `.github/copilot-instructions.md` 与 `local_config/yu_datacenter.yaml`）。
- 金融数值优先 `rust_decimal`，避免 `f64`（workspace 依赖统一声明）。

## 5) 跨组件集成方式（推荐复用）
- 事件分发统一用 Actix 订阅宏：`subscribe_event_addr!`（`li/src/tools/pubsub.rs`）。
- WebSocket 订阅与存储链路参考：`yu/src/binance/jobs.rs` 中 `start_spot_websocket_stream_job()`。
- 新增交易所接入时，优先在 `yue/` 做协议适配，在 `yu/` 做调度与落库，`li/` 仅放通用能力。
