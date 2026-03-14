# BinanceRestfulClient 整合——设计文档

## 实现的功能

1. 将已 deprecated 的 `YueRequest` 体系的请求构建、限流检查、响应反序列化能力融入 `BinanceRestfulClient`
2. `BinanceRestfulClient` 成为币安 HTTP 请求的唯一入口，内建 429/418 重试、x-mbx-used-weight 监控、网络瞬时错误重试
3. 签名/非签名请求通过 `public()` / `signed()` 两种构造方式区分，不再需要外部传入 `YueRequestBuilder` 泛型
4. 保留现有 `RequestInfo` + `LazyLock` 命令定义体系，每个 API 自控 weight 和 limiter 引用（多 API 可共享同一个 static limiter）
5. 精简 `yue/src/http_client.rs`，删除 `YueRequest`、`ResponseHandler`、`YueRequestBuilder`、`NonAuthRequestBuilder`
6. 为后续新增交易所（OKX 等）提供可参考的模式：每个交易所独立 Client

## 所有的技术

| 组件 | 技术 | 用途 |
|------|------|------|
| HTTP 客户端 | reqwest（全局 `HTTP_CLIENT`） | 统一代理配置、连接复用 |
| 限流 | governor（`DefaultRateLimiter`） | 令牌桶限流，由 `RequestInfo.rate_limit` 提供 static 引用 |
| 签名 | hmac + sha256（`sign_hmac`） | 币安 HMAC-SHA256 签名 |
| 重试 | 内建循环 + backon（外部重试） | 429/418/网络错误内建重试；业务层可叠加 backon 重试 |
| 参数序列化 | `ToQueryParams` trait | query string 构建 |
| 响应反序列化 | serde_json | JSON → 泛型 `U: DeserializeOwned` |

## 流程图

### 整体架构数据流向

```
调用方（history_data / order_book / listen_key_client / bn_mcp / examples）
    │
    │  client.get(&COMMAND, Some(&param))
    │  client.post(&COMMAND, Some(&param), body)
    │  client.retryable_get(&COMMAND, &param).retry(backoff)
    ▼
┌──────────────────────────────────────────────┐
│           BinanceRestfulClient               │
│  ┌────────────────────────────────────────┐  │
│  │ auth: None  →  公开请求               │  │
│  │ auth: Some(BinanceAuth) → 签名请求    │  │
│  └────────────────────────────────────────┘  │
│                                              │
│  execute<P, U>(info, param, body, method)    │
│    │                                         │
│    ├─ ① check_rate_limit                    │
│    │     ↑ info.rate_limit（static共享）      │
│    │     ↑ info.weight                       │
│    │                                         │
│    ├─ ② build_request                       │
│    │     ├─ auth=None → URL + query          │
│    │     └─ auth=Some → HMAC签名 + header    │
│    │     ↑ HTTP_CLIENT（全局单例）             │
│    │                                         │
│    ├─ ③ request.send().await                │
│    │                                         │
│    ├─ ④ 响应处理                            │
│    │     ├─ 429/418 → retry-after/退避重试   │
│    │     ├─ x-mbx-used-weight-1m → 日志告警  │
│    │     ├─ 成功 → serde_json反序列化 → U    │
│    │     └─ 网络错误 → 指数退避重试           │
│    │                                         │
│    └─ ⑤ return Result<U, YueError>          │
└──────────────────────────────────────────────┘
         │                    ▲
         ▼                    │
┌────────────────┐   ┌───────────────────────────────┐
│ HTTP_CLIENT    │   │ RequestInfo (LazyLock)         │
│ (全局 reqwest  │   │  - URL (base + path)           │
│  Client)       │   │  - weight                      │
│                │   │  - rate_limit → &'static       │
│  由            │   │    DefaultRateLimiter           │
│  init_http_    │   │  - timeout                     │
│  client()      │   │                                │
│  初始化        │   │ 多个COMMAND共享同一个limiter     │
│                │   │ 如 SPOT_RATE_LIMITER            │
└────────────────┘   └───────────────────────────────┘
```

### execute 内部重试循环

```
enter execute()
    │
    ▼
┌───────────────────┐
│ rate_limit 检查    │──超时──→ Err(限流超时)
└───────┬───────────┘
        │ 通过
        ▼
┌───────────────────┐
│ build_request()   │──失败──→ Err(构建失败)
└───────┬───────────┘
        │
        ▼
┌───────────────────┐
│ request.send()    │
└───────┬───────────┘
        │
    ┌───┴────────────────────────────┐
    │                                │
  Ok(resp)                      Err(err)
    │                                │
    ├─ status=429/418                ├─ timeout/connect/request
    │   attempt += 1                 │   attempt += 1
    │   attempt > MAX_RETRIES?       │   attempt > MAX_RETRIES?
    │   ├─ Y → Err(超过重试)         │   ├─ Y → Err(网络错误)
    │   └─ N → parse retry-after     │   └─ N → 指数退避
    │          或 指数退避+jitter     │          sleep → loop
    │          sleep → loop          │
    │                                ├─ 其他错误
    ├─ status=2xx                    │   → Err(other)
    │   log(x-mbx-used-weight-1m)   │
    │   serde_json::from_slice::<U>  │
    │   → Ok(U) 或 Err(SerdeError)  │
    │                                │
    └─ status=4xx/5xx(非429/418)     │
        → Err(ExchangeRequestError)  │
```

## 风险点

| 风险 | 严重度 | 缓解措施 |
|------|--------|---------|
| 迁移期间新旧代码并存导致两条路径同时消耗 rate limit | 中 | 按模块逐个迁移，每步完成后立即删除旧调用；同一 API 不能同时走两条路径 |
| `BinanceRestfulClient` 内建重试 + 外部 `backon` 重试可能导致双重重试 | 中 | 内建重试只处理 429/418 和网络瞬时错误；业务错误（4xx/5xx）直接返回，由外部 backon 决定是否重试 |
| `HTTP_CLIENT` 未初始化时调用 `build_request` panic | 低 | `build_request` 中用 `ok_or(YueError)` 而非 `unwrap()`，返回明确错误 |
| 签名逻辑从 `BNSecurityRequestBuilder` 移入后，HMAC 行为需保持完全一致 | 中 | 复用现有 `sign_hmac` 函数，迁移后保留原有签名单元测试用例验证签名结果不变 |
| `x-mbx-used-weight-1m` 监控降级为日志后丢失主动限流能力 | 低 | 当前 weight 监控只做 `sleep(1s)`，实际限流由 governor 令牌桶保证；日志告警足以辅助运维 |

## 设计的模块和组件

### 模块划分

```
yue/src/
├── http_client.rs            ← 薄工具层（保留）
│   ├── HTTP_CLIENT            全局 reqwest Client
│   ├── init_http_client()     初始化（代理配置）
│   ├── DefaultRateLimiter     type alias
│   ├── check_rate_limit()     通用限流检查
│   └── ClonableResponseCache  响应缓存（重试用）
│
├── binance/
│   ├── http_client.rs         ← 核心改造
│   │   ├── BinanceRestfulClient   结构体
│   │   ├── BinanceAuth            内部签名凭证
│   │   ├── public()               公开请求构造
│   │   ├── signed()               签名请求构造
│   │   ├── get/post/put()         请求方法
│   │   ├── retryable_get()        backon 兼容
│   │   ├── execute()              内部执行循环
│   │   └── build_request()        请求构建（含签名）
│   │
│   ├── bn_restful_commands.rs ← 精简
│   │   ├── URL/PATH 常量          不变
│   │   ├── define_rate_limiter!   不变
│   │   ├── LazyLock<RequestInfo>  不变
│   │   └── 删除: BNSecurityRequestBuilder
│   │         BinanceResponseHandler
│   │         execute_bn_get/post/put
│   │
│   ├── history_data.rs        ← 适配调用方式
│   ├── order_book.rs          ← 适配调用方式
│   └── listen_key_client.rs   ← 适配调用方式
```

### BinanceRestfulClient 结构

```rust
#[derive(Clone)]
pub struct BinanceRestfulClient {
    auth: Option<BinanceAuth>,
}

#[derive(Clone)]
struct BinanceAuth {
    api_key: String,
    api_secret: String,
}
```

**关键决策**：
- 不自持 `Client` → 复用全局 `HTTP_CLIENT`（统一代理配置）
- 不自持 `RateLimiter` → 限流由每个 `RequestInfo.rate_limit` 提供（static，多 API 共享同一个 limiter）
- 不自持 `weight_limit` → weight 监控降级为日志告警

### 核心方法签名

```rust
impl BinanceRestfulClient {
    pub fn public() -> Self;
    pub fn signed(api_key: String, api_secret: String) -> Self;

    pub async fn get<P: ToQueryParams, U: DeserializeOwned + Send>(
        &self, info: &RequestInfo, param: Option<&P>,
    ) -> Result<U, YueError>;

    pub async fn post<P: ToQueryParams, U: DeserializeOwned + Send>(
        &self, info: &RequestInfo, param: Option<&P>, body: Option<&Value>,
    ) -> Result<U, YueError>;

    pub async fn put<P: ToQueryParams, U: DeserializeOwned + Send>(
        &self, info: &RequestInfo, param: Option<&P>, body: Option<&Value>,
    ) -> Result<U, YueError>;

    /// 返回闭包，兼容 backon 的 .retry()
    pub fn retryable_get<'a, P, U>(
        &'a self, info: &'a RequestInfo, param: &'a P,
    ) -> impl FnMut() -> Pin<Box<dyn Future<Output = Result<U, YueError>> + Send + 'a>> + 'a
    where
        P: ToQueryParams + Sync,
        U: DeserializeOwned + Send + 'static;
}
```

### 调用方迁移对照

| 场景 | 迁移前 | 迁移后 |
|------|--------|--------|
| 公开 GET | `execute_bn_get::<EP, NonAuth, ServerTime>(&CMD, None, NonAuth{}).execute().await?` | `BinanceRestfulClient::public().get::<EP, ServerTime>(&CMD, None).await?` |
| 带参数 GET | `execute_bn_get::<CP, NonAuth, Ticker24hr>(&CMD, Some(&p), NonAuth{}).execute().await?` | `BinanceRestfulClient::public().get::<CP, Ticker24hr>(&CMD, Some(&p)).await?` |
| 签名 POST | `execute_bn_post::<CP, BNSec, LKR>(info, None, None, BNSec::new(k,s)).execute().await?` | `BinanceRestfulClient::signed(k,s).post::<CP, LKR>(info, None, None).await?` |
| 带重试 GET | `execute_bn_get::<T, NonAuth, Vec<O>>(&info, Some(&p), rb).into_retryable().retry(bp).await?` | `BinanceRestfulClient::public().retryable_get::<T, Vec<O>>(&info, &p).retry(bp).await?` |

## 备选方案

### 方案B：保留 trait 体系，BinanceRestfulClient 实现通用 trait

```rust
#[async_trait]
trait ExchangeRestfulClient {
    async fn get<P, U>(&self, info: &RequestInfo, param: Option<&P>) -> Result<U, YueError>;
    async fn post<P, U>(&self, info: &RequestInfo, param: Option<&P>, body: Option<&Value>) -> Result<U, YueError>;
}
```

- **优点**：可以在 yu 层通过 trait object 做交易所无关编程
- **缺点**：
  - `async fn` 在 trait 中需要 `async_trait` 宏，有额外 Box 开销
  - 泛型方法（`<P, U>`）无法做 trait object（`dyn ExchangeRestfulClient`），除非用 type erasure
  - 当前项目只有币安一个交易所，过早抽象违反 YAGNI 原则
- **结论**：暂不采用。当引入第二个交易所时，再从具体实现中提炼公共 trait

### 方案C：保留 YueRequest，在其上包装 BinanceRestfulClient

- **优点**：改动最小
- **缺点**：
  - `YueRequest` 的 4 泛型参数复杂度不减
  - `BinanceRestfulClient` 的 429/418 重试无法与 `YueRequest` 的重试协调
  - `YueRequest` 已标记 deprecated，继续维护两套增加认知成本
- **结论**：不采用

## 可能的扩展

1. **OKX 交易所**：按同样模式创建 `yue/src/okx/http_client.rs` → `OkxRestfulClient`，实现自己的签名方式（OKX 使用 `HMAC-SHA256 + Base64`）、限流 header（`x-ratelimit-*`）、错误格式
2. **通用 trait 提炼**：当引入第二个交易所时，从 `BinanceRestfulClient` 和 `OkxRestfulClient` 中提炼公共接口
3. **weight 监控增强**：若需主动限流（而非仅日志），可恢复 `weight_limit: AtomicU32` 字段，在 `execute()` 中读取 header 后动态调整等待时间
4. **请求指标收集**：在 `execute()` 中埋点（请求耗时、重试次数、状态码分布），暴露给 Prometheus 或日志系统

