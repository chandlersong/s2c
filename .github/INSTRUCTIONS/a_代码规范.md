---
name: coding-standards
description: Universal coding standards, best practices, and patterns for TypeScript, JavaScript, React, and Node.js development.
---

# 编码规范与最佳实践

适用于所有项目的通用编码规范。

## 代码质量原则

### 1. 可读性优先

- 代码被读的次数多于被写的次数
- 使用清晰的变量名和函数名
- 优先使用自说明代码而不是注释
- 保持格式一致

### 2. KISS（保持简单，别搞复杂）

- 使用能工作的最简单方案
- 避免过度设计
- 不要过早优化
- 易于理解优于所谓“聪明”的写法

### 3. DRY（不要重复自己）

- 将公共逻辑提取为函数
- 创建可复用组件
- 在模块间共享工具函数
- 避免拷贝粘贴式编程

### 4. YAGNI（你不会需要它）

- 不要在还没需要时就实现功能
- 避免投机性的泛化
- 仅在必要时增加复杂性
- 从简单开始，需要时重构

## TypeScript/JavaScript 规范

### 变量命名

```rust
// ✅ 良好：描述性命名（Rust 风格为 snake_case）
let market_search_query = "election";
let is_user_authenticated: bool = true;
let total_revenue: i64 = 1000;

// ❌ 不好：命名不明确
let q = "election";
let flag = true;
let x = 1000;
```

### 函数命名

```rust
// ✅ 良好：动词-名词模式（snake_case）
async fn fetch_market_data(market_id: &str) -> Result<Market, Box<dyn std::error::Error>> {
    // 实现
    Ok(Market { id: market_id.to_string(), name: "".into(), status: Status::Active, created_at: chrono::Utc::now() })
}

fn calculate_similarity(a: &[f64], b: &[f64]) -> f64 {
    // 实现
    0.0
}

fn is_valid_email(email: &str) -> bool {
    // 简单示例
    email.contains('@')
}

// ❌ 不好：不清晰或仅用名词
fn market(id: &str) {}
fn similarity(a: &[f64], b: &[f64]) {}
fn email(e: &str) {}
```

### 不可变模式（关键）

```rust
// ✅ 推荐：优先使用不可变绑定和结构体更新语法
#[derive(Clone)]
struct User {
    name: String,
    age: u32
}

let user = User { name: "Alice".into(), age: 30 };
let updated_user = User { name: "New Name".into(), ..user.clone() };

// 如果确实需要可变性，显式声明为 mut
let mut user_mut = user.clone();
user_mut.name = "New Name".into(); // 谨慎使用
```

### 错误处理

```rust
// ✅ 良好：使用 Result 和 ? 运算符进行错误传播
use serde_json::Value;

async fn fetch_data(url: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let resp = reqwest::get(url).await?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}: {}", resp.status(), resp.text().await?).into());
    }
    let json = resp.json::<Value>().await?;
    Ok(json)
}

// ❌ 不好：忽略错误
async fn fetch_data_unchecked(url: &str) -> serde_json::Value {
    let resp = reqwest::get(url).await.unwrap();
    resp.json().await.unwrap()
}
```

### Async/Await 最佳实践

```rust
// ✅ 良好：并发执行多个异步任务（使用 tokio）
use tokio::try_join;

async fn run_concurrent() -> Result<(Users, Markets, Stats), Box<dyn std::error::Error>> {
    let (users, markets, stats) = try_join!(fetch_users(), fetch_markets(), fetch_stats())?;
    Ok((users, markets, stats))
}

// ❌ 不好：不必要的顺序执行
async fn run_sequential() -> Result<(), Box<dyn std::error::Error>> {
    let users = fetch_users().await?;
    let markets = fetch_markets().await?;
    let stats = fetch_stats().await?;
    Ok(())
}
```

### 类型安全

```rust
// ✅ 良好：使用 struct 和 enum 明确定义类型
use chrono::{DateTime, Utc};

struct Market {
    id: String,
    name: String,
    status: MarketStatus,
    created_at: DateTime<Utc>,
}

enum MarketStatus { Active, Resolved, Closed }

async fn get_market(id: &str) -> Result<Market, Box<dyn std::error::Error>> {
    // 实现
    Ok(Market { id: id.to_string(), name: "".into(), status: MarketStatus::Active, created_at: Utc::now() })
}

// ❌ 不好：使用动态类型或大量 unwrap
async fn get_market_untyped(id: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    // 实现
    Ok(serde_json::json!({}))
}
```

### 尽量多的实现Default的trait，参数多用Option
目的：
1. 正式环境下，用正式的代码

```rust
use yue::tools::get_snow_flake_id_u64;

struct Example {
    id: u64,
}

impl Default for Example{
    fn default() -> Self {
        Example {
            id: get_snow_flake_id_u64(),
        }
    }
}

fn use_example(example: Option<Example>) {
    let example = example.unwrap_or_default();
    println!("Example ID: {}", example.id);
}
```

### 状态管理

```rust
// ✅ 良好：使用原子类型或互斥锁来管理共享状态
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

let count = Arc::new(AtomicUsize::new(0));
let c = Arc::clone( & count);
// 基于先前状态的原子更新
c.fetch_add(1, Ordering::SeqCst);

// 使用 Mutex 在异步上下文中保护复杂数据
use tokio::sync::Mutex;
let state = Arc::new(Mutex::new(0i32));
{
let mut s = state.lock().await;
* s += 1;
}
```

### 条件渲染 / 分支处理

```rust
// ✅ 良好：使用 Option 和匹配来代替链式三元表达式
fn render(is_loading: bool, error: Option<&str>, data: Option<&str>) {
    if is_loading {
        println!("Spinner");
        return;
    }
    if let Some(e) = error {
        eprintln!("Error: {}", e);
        return;
    }
    if let Some(d) = data {
        println!("Data: {}", d);
    }
}
```

## API 设计规范

### REST API 约定

```
GET    /api/markets              # 列出所有市场
GET    /api/markets/:id          # 获取指定市场
POST   /api/markets              # 创建新市场
PUT    /api/markets/:id          # 更新市场（整体）
PATCH  /api/markets/:id          # 更新市场（部分）
DELETE /api/markets/:id          # 删除市场

# 过滤用的查询参数
GET /api/markets?status=active&limit=10&offset=0
```

### 响应格式

```rust
// ✅ 良好：一致的响应结构，使用 serde 序列化
use serde::Serialize;

#[derive(Serialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
    meta: Option<Meta>,
}

#[derive(Serialize)]
struct Meta {
    total: usize,
    page: usize,
    limit: usize
}

// 成功响应（伪代码）
// 返回 JSON: ApiResponse { success: true, data: Some(markets), meta: Some(Meta { ... }) }

// 错误响应（伪代码）
// ApiResponse { success: false, error: Some("Invalid request".into()), data: None }
```

### 输入校验

```rust
use serde::Deserialize;

// ✅ 良好：使用 serde + 手工/第三方校验
#[derive(Deserialize)]
struct CreateMarket {
    name: String,
    description: String,
    end_date: String,
    categories: Vec<String>,
}

fn validate_create_market(m: &CreateMarket) -> Result<(), String> {
    if m.name.is_empty() || m.name.len() > 200 { return Err("name invalid".into()); }
    if m.description.is_empty() { return Err("description invalid".into()); }
    if m.categories.is_empty() { return Err("categories empty".into()); }
    Ok(())
}

// 在处理请求时先反序列化，再校验
// let body: CreateMarket = serde_json::from_str(&body_str)?;
// validate_create_market(&body)?;
```

## 文件组织

### 项目结构

```
src/
├── app/                    # Next.js 对应的 Rust web 框架目录（如 actix-web/axum）
│   ├── api/               # API 路由
│   ├── markets/           # 市场相关模块
│   └── auth/              # 认证模块
├── components/            # 如果使用 Yew，可放置组件
├── hooks/                 # 工具性模块
├── lib/                  # 工具与配置
│   ├── api/             # API 客户端
│   ├── utils/           # 辅助函数
│   └── constants/       # 常量
├── types/                # 领域类型定义
└── styles/               # 前端样式（若使用 WASM 前端）
```

### 文件命名

```
components/button.rs          # 组件使用 snake_case
hooks/use_auth.rs             # 钩子/工具使用 snake_case
lib/format_date.rs            # 工具使用 snake_case
types/market_types.rs         # 类型文件
```

## 注释与文档

### 何时添加注释

```rust
// ✅ 良好：解释为什么，而不是做了什么
// 使用指数退避以避免在 API 故障期间过度冲击服务
let delay = std::cmp::min(1000u64 * 2u64.pow(retry_count), 30_000u64);

// 在这里故意使用可变以在大量数据场景下提升性能
let mut items = Vec::new();
items.push(new_item);

// ❌ 不好：陈述显而易见的事情
// 计数加 1
count += 1;

// 将 name 设为用户的名字
name = user.name.clone();
```

### 公共 API 的文档注释

```rust
/// 使用语义相似度搜索市场。
///
/// # 参数
///
/// - `query` - 自然语言搜索查询
/// - `limit` - 最大返回数量（默认：10）
///
/// # 返回
///
/// 按相似度分数排序的市场数组
///
/// # 错误
///
/// 如果 OpenAI API 失败或 Redis 不可用，将返回错误
///
/// # 示例
///
/// ```rust
/// let results = search_markets("election", 5).await?;
/// println!("{}", results[0].name);
/// ```
async fn search_markets(query: &str, limit: usize) -> Result<Vec<Market>, Box<dyn std::error::Error>> {
    // 实现
    Ok(vec![])
}
```

## 性能最佳实践

### 记忆化（Memoization）

```rust
use once_cell::sync::OnceCell;

// ✅ 良好：对昂贵计算进行一次性缓存
static SORTED_MARKETS: OnceCell<Vec<Market>> = OnceCell::new();

fn get_sorted_markets(markets: Vec<Market>) -> &'static Vec<Market> {
    SORTED_MARKETS.get_or_init(|| {
        let mut m = markets;
        m.sort_by(|a, b| b.volume.cmp(&a.volume));
        m
    })
}

// 或者使用 cached 等第三方库进行更灵活的缓存策略
```

### 懒加载

```rust
use once_cell::sync::Lazy;

// ✅ 良好：惰性初始化重量级资源
static HEAVY_CHART: Lazy<String> = Lazy::new(|| {
    // 假设这是一个昂贵的构建过程
    "HeavyChart资源已加载".to_string()
});

fn render_dashboard() {
    println!("{}", *HEAVY_CHART);
}
```

### 数据库查询

```rust
// ✅ 良好：仅选择必要的列（以 sqlx 为例）
let rows = sqlx::query!("SELECT id, name, status FROM markets LIMIT 10")
.fetch_all( & pool)
.await?;

// ❌ 不好：选择全部字段
let rows = sqlx::query!("SELECT * FROM markets")
.fetch_all( & pool)
.await?;
```

## 测试规范

### 测试结构（Arrange/Act/Assert）

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_similarity_correctly() {
        // Arrange
        let vector1 = vec![1.0, 0.0, 0.0];
        let vector2 = vec![0.0, 1.0, 0.0];

        // Act
        let similarity = calculate_cosine_similarity(&vector1, &vector2);

        // Assert
        assert_eq!(similarity, 0.0);
    }
}
```

### 测试命名

```rust
// ✅ 良好：描述性测试名称
#[test]
fn returns_empty_vec_when_no_markets_match_query() {}

#[test]
fn throws_error_when_openai_api_key_is_missing() {}

#[test]
fn falls_back_to_substring_search_when_redis_unavailable() {}

// ❌ 不好：含糊的测试名称
#[test]
fn works() {}

#[test]
fn test_search() {}
```

## 代码异味检测

注意以下反模式：

### 1. 长函数

```rust
// ❌ 不好：函数过长，超过可维护范围
fn process_market_data() {
    // 100 行代码
}

// ✅ 良好：拆分为更小的函数
fn process_market_data() {
    let validated = validate_data();
    let transformed = transform_data(&validated);
    save_data(&transformed);
}
```

### 2. 深度嵌套

```rust
// ❌ 不好：嵌套过深
if let Some(user) = get_user() {
if user.is_admin {
if let Some(market) = get_market_option() {
if market.is_active {
if has_permission() {
// 做事情
}
}
}
}
}

// ✅ 良好：提前返回与模式匹配减少嵌套
fn do_action() {
    let user = match get_user() {
        Some(u) => u,
        None => return
    };
    if !user.is_admin { return; }
    let market = match get_market_option() {
        Some(m) => m,
        None => return
    };
    if !market.is_active { return; }
    if !has_permission() { return; }
    // 做事情
}
```

### 3. 魔法数字

```rust
// ❌ 不好：无说明的数字
if retry_count > 3 {}
std::thread::sleep(std::time::Duration::from_millis(500));

// ✅ 良好：使用具名常量
const MAX_RETRIES: u8 = 3;
const DEBOUNCE_DELAY_MS: u64 = 500;

if retry_count > MAX_RETRIES { }
std::thread::sleep(std::time::Duration::from_millis(DEBOUNCE_DELAY_MS));
```

**牢记**：代码质量不容妥协。清晰且易维护的代码能加速开发与自信地重构。
