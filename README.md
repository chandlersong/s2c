# 简介

s2c的意思是Strategy to Cloud。最终的目的是希望能够写一个策略。可以一步部署到云端。

第一阶段的方向是能够在币安上跑一些简单的策略。

# 模块介绍

参照[君子六艺](https://zh.wikipedia.org/wiki/%E5%85%AD%E8%89%BA)为主要模块
- 礼：纯技术的基础事物。该模块主要负责，诸如通知，数据库访问之类的纯技术的代码。
- 乐：主要负责和交易所的沟通事宜。
- 御：回测相关的代码
  - 鸣和鸾： 一个给AI agent提供相应数据的mcp

# Flight SQL Server

项目包含一个基于 DuckDB 和 Arrow Flight 的 Flight SQL 服务器实现。

## 运行服务器

```bash
cargo run --bin flight_sql_server
```

服务器将在 `0.0.0.0:8815` 上启动。

## 测试服务器

```bash
cargo run --bin flight_sql_test_client
```

这将启动服务器，等待几秒钟，然后停止它以验证服务器可以正常启动和停止。

## Flight SQL 功能

服务器实现了以下 Flight SQL 功能：
- SQL 查询执行 (`SELECT` 语句)
- 结果以 Arrow 格式返回
- 支持基本的元数据查询
- 基于 DuckDB 的高性能查询处理


