# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

s2c（Strategy to Cloud）是一个 Rust 量化交易系统，目标是将交易策略一键部署到云端。当前阶段聚焦于币安交易所的数据采集与策略执行。

## 架构分层（Cargo Workspace）

依赖方向固定：**yu → yue → li**，且 yu → li 允许；禁止反向依赖。

| Crate | 名称 | 职责 |
|-------|------|------|
| `li/` | 礼（基础设施层） | RocksDB、WebSocket 连接抽象、通知（Telegram）、AWS、定时任务 |
| `yue/` | 乐（交易所层） | 币安 REST/WebSocket、HTTP 客户端、模型转换、签名认证、限流 |
| `yu/` | 御（应用层） | 配置管理、DuckDB 存储、数据完整性修复、Arrow Flight 服务、调度编排 |

## 构建 / 测试 / 运行

```bash
# 全量编译
cargo build --workspace

# 全量测试（CI 同款）
cargo test --all-features

# 运行单个测试
cargo test -p yu <test_name>

# 运行数据中心（主服务）
cargo run -p yu --bin yu_datacenter

# 运行 MCP 服务
cargo run -p yu --bin yu_mcp

# 运行示例
cargo run -p yu --example duckdb_example
cargo run -p yu --example data_integrity_example
cargo run -p yu --example bn_spot_stream_example
cargo run -p yue --example bn_restful_examples
cargo run -p yue --example order_book_example
```

## 核心约定

1. **交易对必须全大写**：`BTCUSDT`、`ETHUSDT`，不允许小写。
2. **金融数值用 `rust_decimal::Decimal`**，禁止用 `f64` 做金额/价格计算。
3. **错误按三层体系**：`LiError`(li) → `YueError`(yue) → `YuError`(yu)，各 crate 的 `errors.rs` 中定义。
4. **配置用环境变量覆盖**：`CONFIG_PATH` 指定配置文件路径，`YU_` 前缀的环境变量用下划线映射嵌套键（如 `YU_PROXYURL`、`YU_DATABASE_PATH`）。见 `yu/src/config.rs`。
5. **ID 生成**：用 `yue::tools::get_snow_flake_id_u64()` 生成分布式唯一 ID。
6. **参数多用 `Option`**，结构体尽量实现 `Default` trait，正式环境用正式代码，测试环境用 mock。
7. **所有回答用中文**，除非用户要求英文。
8. **不允许修改用户写的注释和已有说明文档**。

## 关键数据流

### 数据中心入口 (`yu_datacenter`)
`yu/src/bin/yu_datacenter.rs` 启动顺序：
1. 读取配置 `get_config()` → 初始化日志 `setup_logger()` → 初始化 HTTP 客户端 `init_http_client()`
2. `start_bn_jobs()`：初始化 Dashboard → 创建 DuckDB 表 → 启动 Spot/Swap Kline WebSocket → 回补历史 Kline → 同步 Funding Rate → 启动数据完整性修复
3. 启动 Arrow Flight 服务 `0.0.0.0:8815`
4. Ctrl+C 后通过 `System::current().stop()` 优雅退出


## WebSocket 数据流

核心抽象在 `li/src/websocket/connection.rs`：
- **`WebSocketConnection`** / **`WebSocketInterface`** / **`MessageHandlerTrait`**（Tokio 模式）
- 业务处理实现 `MessageHandlerTrait`（如 `SpotKlineSaver`、`SwapKlineSaver`）
- `WebSocketConnection` 读取 `WsMessage::Text/Binary` 后调用 `M::from_text` / `M::from_binary`，再交给 handler
- 连接控制通过 `WebSocketInterface::command_sender()` 发送 `CommandMessage`，常用 `CommandMessage::Connection(ConnectionAction::Close)`
- `WebSocketEvent`（`Connected`/`Disconnected`/`Reconnecting`/`Error`）仅用于连接态广播，不承载行情消息

**注意**：`li/src/websocket/client.rs` 已标记 `#[deprecated]`，是旧的 Actix 订阅式链路，新代码不要使用。

## 数据库

- **DuckDB**（列存，时间序列）：`yu/src/duck_db.rs` → `DBProvider`，`database.path` 未配置时退回内存库
- **RocksDB**（KV 存储）：`li/src/rocksdb/` → `RollingKVDB`（按天轮转）。Key 命名规范见 `DB_Reference.MD`。
- DuckDB 表定义集中在 `yu/src/duck_db_tables.rs`。

## HTTP 请求

- 入口：`yu/src/binance/jobs.rs` 中 `init_http_client()` 初始化带重试和限流的 HTTP 客户端代理
- 执行请求：`yue::binance::bn_restful_commands::execute_bn_get` 系列函数
- 限流通过 `governor` 实现

## 代码风格

- `rustfmt.toml`：`max_width = 150`
- Rust edition 2024
- 优先不可变绑定，使用 `Result` + `?` 传播错误
- 公开 API 使用文档注释（`///`），说明参数、返回值、错误和示例
