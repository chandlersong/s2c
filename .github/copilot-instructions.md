# S2C 项目 Copilot 开发指导

> 更新于 2026-01-23
> 详细文档存放在 `.github/INSTRUCTIONS/` 目录中

#  基本行为
1) 默认行为：
    - 所有设计，任务，需求的模块，与cargo对应
    - 所有回答用中文
    - 除非用户要求，否则不允许修改任何的已经存在的说明文档，包括但不限于makrdown，注释，设计文档等
2) 所有角色要求：
    - 熟悉币安，OKEX等加密货币交易所的API
    - 熟悉币安，OKEX等加密货币交易所的交易规则
    - 了解主流的量化交易策略
    - 熟悉各种金融衍生品的交易规则
    - 熟悉期权，期货的一些公开策略。比如费率逃离，蝶式期权这类。
3) 业务的规范
    - 所有的交易对都是大写，例如BTCUSDT

# 不允许行为
1. 修改，删除用户写的注释。

---

## 📍 详细文档位置

所有开发指导文档都在 `.github/INSTRUCTIONS/` 目录中，我会根据任务主动读取相关文档。

## 🚀 快速任务导航

当遇到以下任务时，我会自动读取相关文档：

| 任务类型         | 对应文档 |
|--------------|---------|
| 基本开发准则       | `.github/INSTRUCTIONS/a_代码规范.md` |
| 项目规范         | `.github/project.instructions.md` |
| 文档规范         | `.github/doc.instructions.md` |
| WebSocket 相关 | `.github/INSTRUCTIONS/04_WebSocket数据流.md` |
| HTTP 请求      | `.github/INSTRUCTIONS/05_HTTP请求框架.md` |
| 数据库操作        | `.github/INSTRUCTIONS/03_数据库操作.md` |
| 定义数据模型       | `.github/INSTRUCTIONS/02_领域对象定义.md` |
| 定义 Actor     | `.github/INSTRUCTIONS/06_Actor模式.md` |
| 错误处理         | `.github/INSTRUCTIONS/07_错误处理.md` |
| 配置管理         | `.github/INSTRUCTIONS/08_配置管理.md` |
| 架构问题         | `.github/INSTRUCTIONS/01_架构概览.md` |

## 🏗️ 架构速查

### 三层架构
```
yu（御）- 应用层：业务逻辑、数据存储、订阅者
  ↓ 依赖
yue（乐）- 交易所层：REST API、WebSocket、签名认证
  ↓ 依赖
li（礼）- 基础设施层：RocksDB、定时任务、通知、AWS
```

### 核心技术栈
- **异步运行时**：Tokio + Actix
- **WebSocket**：tokio-tungstenite
- **HTTP**：reqwest + governor（限流）
- **数据库**：DuckDB（列存）+ RocksDB（KV）
- **数值**：rust_decimal（金融级精度）
- **签名**：hmac + ed25519
- **ID**：sonyflake（分布式唯一ID）

## 📋 核心概念速查

| 概念 | 说明 | 详见 |
|------|------|------|
| **VO** | 从API响应反序列化的对象 | 02_领域对象定义.md |
| **PO** | 存储到数据库前的持久化对象 | 02_领域对象定义.md |
| **Decimal** | 金融级精度数值类型 | 02_领域对象定义.md |
| **WebSocketClient** | WebSocket 连接管理 | 04_WebSocket数据流.md |
| **WsMessageBus** | 事件总线：解析+广播 | 04_WebSocket数据流.md |
| **Actor** | Actix 并发单位 | 06_Actor模式.md |
| **DBProvider** | DuckDB 连接池管理 | 03_数据库操作.md |
| **RollingKVDB** | 按天轮转的 RocksDB | 03_数据库操作.md |

## 💾 核心 API 速查

### 配置获取
```rust
use yu::config::get_config;
let config = get_config();
```

### 数据库连接
```rust
use yu::duck_db::DBProvider;
let db = DBProvider::default();
let conn = db.acquire()?;
```

### HTTP 请求
```rust
use yue::binance::bn_restful_commands::execute_bn_get;
let result = execute_bn_get(&COMMAND, Some(&params), builder).execute().await?;
```

### WebSocket 启动（三步）
```rust
// 1. 启动客户端
let client = WebSocketClient::new(url).start();

// 2. 创建总线和订阅者
let bus = WsMessageBus::new(parser).start();
let subscriber = MySubscriber.start();

// 3. 连接
bus.do_send(Subscribe { subscriber: subscriber.recipient() });
client.send(SubscribeToEvents { recipient: bus.recipient() }).await?;
```

### 定义 Actor
```rust
impl Handler<Message> for MyActor {
    type Result = ();
    fn handle(&mut self, msg: Message, _ctx: &mut Context<Self>) {
        // 处理消息
    }
}
```

### 错误处理
```rust
use yue::errors::YueError;
Err(YueError::new("错误信息"))?
```

## ✅ 业务规范检查清单

开发时必须遵守：

- [ ] **交易对全大写**：BTCUSDT、ETHUSDT（不能小写）
- [ ] **数值用 Decimal**：所有金额/价格必须用 `Decimal`，不能用 `f64`
- [ ] **错误按三层体系**：YueError(yue) → YuError(yu) → LiError(li)
- [ ] **配置从环境变量读取**：支持 `YU_` 前缀的环境变量覆盖
- [ ] **使用 Snowflake ID**：`get_snow_flake_id_u64()` 生成唯一ID

## 📚 完整文档列表

1. `.github/INSTRUCTIONS/00_快速开始.md` - 导航和速查表
2. `.github/INSTRUCTIONS/01_架构概览.md` - 项目整体设计
3. `.github/INSTRUCTIONS/02_领域对象定义.md` - VO/PO/DTO 规范
4. `.github/INSTRUCTIONS/03_数据库操作.md` - DuckDB/RocksDB 用法
5. `.github/INSTRUCTIONS/04_WebSocket数据流.md` - 实时数据处理
6. `.github/INSTRUCTIONS/05_HTTP请求框架.md` - REST API 调用
7. `.github/INSTRUCTIONS/06_Actor模式.md` - Actix 框架详解
8. `.github/INSTRUCTIONS/07_错误处理.md` - 错误管理策略
9. `.github/INSTRUCTIONS/08_配置管理.md` - 配置与环境变量

## 🤖 我的工作方式

当你提出问题时，我会：
1. 根据任务类型判断需要哪个详细文档
2. 主动读取 `.github/INSTRUCTIONS/` 中对应的文档
3. 结合代码上下文和详细文档给出答案

例如：
- **你问**："怎么处理 WebSocket？"
- **我会**：读取 `04_WebSocket数据流.md`，然后给你完整的启动步骤和代码示例



