# HTTP 请求框架指南

## 整体流程

```
YueRequest<Builder, ResponseType, Handler>
    ↓
检查限流 (governor)
    ↓
compose_request (Builder 负责签名和头部)
    ↓
发送 HTTP 请求 (reqwest)
    ↓
handle_response (Handler 负责响应处理)
    ↓
返回结果
```

## 初始化 HTTP 客户端

### 全局 HTTP 客户端

```rust
use yue::http_client::init_http_client;

// 在应用启动时调用（只需调用一次）
let proxy = Some("http://127.0.0.1:7891");
init_http_client(proxy);

// 或无代理
init_http_client(None);
```

### 获取客户端

```rust
use yue::http_client::HTTP_CLIENT;

let client = HTTP_CLIENT.get().unwrap();
```

## 定义 Request Builder

### 无身份验证（GET 请求）

```rust
use yue::http_client::NonAuthRequestBuilder;
use yue::models::RequestInfo;
use reqwest::Method;

let builder = NonAuthRequestBuilder::new();

// 使用示例
let request_info = RequestInfo::from_base_path(
    "https://api.binance.com",
    "/api/v3/time",
    false,  // no security
    1,      // weight
    None,   // rate_limiter
    Some(1000),  // timeout_ms
    Some(2),     // rate_limit_timeout_secs
)?;

let builder = builder.compose_request(
    &client,
    &request_info,
    None,  // no params
    Method::GET
)?;
```

### 有身份验证（HMAC-SHA256）

```rust
use yue::http_client::YueRequestBuilder;
use yue::binance::bn_restful_commands::BNSecurityRequestBuilder;

let builder = BNSecurityRequestBuilder {
    api_key: "your_api_key".to_string(),
    api_secret: "your_api_secret".to_string(),
};

// 自动添加 signature 和 X-MBX-APIKEY 头
```

### 自定义 Builder

```rust
use yue::http_client::YueRequestBuilder;
use reqwest::{Client, Method, RequestBuilder};
use yue::models::RequestInfo;
use yue::errors::YueError;

#[derive(Clone)]
pub struct CustomBuilder {
    pub auth_header: String,
}

impl YueRequestBuilder for CustomBuilder {
    fn compose_request(
        &self,
        client: &Client,
        info: &RequestInfo,
        param: Option<String>,
        method: Method,
    ) -> Result<RequestBuilder, YueError> {
        let mut url = info.as_ref().clone();
        
        if let Some(p) = param {
            url.set_query(Some(&p));
        }
        
        let mut request = client.request(method, url.to_string());
        request = request.header("Authorization", &self.auth_header);
        
        Ok(request)
    }
}
```

## 定义 Response Handler

### 简单 JSON 响应

```rust
use yue::http_client::{ResponseHandler, ClonableResponseCache};
use serde::de::DeserializeOwned;
use yue::errors::YueError;
use async_trait::async_trait;

#[derive(Clone)]
pub struct SimpleJsonHandler;

#[async_trait]
impl<U: DeserializeOwned + Send + Sync> ResponseHandler<U> for SimpleJsonHandler {
    async fn handle_response(&self, resp: ClonableResponseCache) -> Result<U, YueError> {
        let result = serde_json::from_slice::<U>(&resp.body)?;
        Ok(result)
    }
}
```

### 币安 API 响应处理

```rust
use yue::binance::bn_restful_commands::BinanceResponseHandler;

#[async_trait]
impl<U: DeserializeOwned + Send + Sync> ResponseHandler<U> for BinanceResponseHandler {
    async fn handle_response(&self, resp: ClonableResponseCache) -> Result<U, YueError> {
        // 检查 HTTP 状态
        if !resp.status.is_success() {
            let body_str = String::from_utf8_lossy(&resp.body);
            return Err(YueError::ExchangeRequestError {
                code: resp.status.as_u16(),
                body: body_str.to_string(),
            });
        }
        
        // 反序列化
        serde_json::from_slice::<U>(&resp.body)
            .map_err(|e| YueError::SerdeError(e))
    }
}
```

## 定义 RequestInfo

### 构建请求信息

```rust
use yue::models::RequestInfo;
use yue::binance::bn_restful_commands::{
    BINANCE_SPOT_API,
    SPOT_KLINE_PATH,
    get_bn_spot_limit,
};

// 方式1：完整 URL
let info = RequestInfo::new_full_url(
    "https://api.binance.com/api/v3/klines",
    false,                    // 无安全认证
    2,                        // 权重为2
    get_bn_spot_limit(),      // 限流器
    Some(1000),               // 超时 1000ms
    Some(60 * 60),            // 限流超时 1小时
)?;

// 方式2：base + path（推荐）
let info = RequestInfo::from_base_path(
    BINANCE_SPOT_API,
    SPOT_KLINE_PATH,
    false,
    2,
    get_bn_spot_limit(),
    Some(1000),
    Some(60 * 60),
)?;
```

### RequestInfo 属性

```rust
pub struct RequestInfo {
    pub has_security: bool,           // 是否需要签名
    pub weight: u32,                  // API 权重
    pub rate_limit: Option<&'static DefaultRateLimiter>,
    pub request_timeout_mill_secs: u32,
    pub rate_limit_timeout_secs: u64,
}
```

## 发送请求

### 快速方式（推荐）

```rust
use yue::binance::bn_restful_commands::execute_bn_get;
use yue::models::RequestInfo;
use yue::http_client::NonAuthRequestBuilder;
use yue::binance::bn_models::spot_restful::Ticker24hr;
use std::collections::BTreeMap;

let mut params = BTreeMap::new();
params.insert("symbol", "BTCUSDT".to_string());

let result = execute_bn_get::<
    BTreeMap<&str, String>,
    NonAuthRequestBuilder,
    Ticker24hr
>(
    &SPOT_TICKER_24HR_ONE_SYMBOL_COMMAND,
    Some(&params),
    NonAuthRequestBuilder::new(),
)
.execute()
.await?;

println!("Price: {}", result.last_price);
```

### 通用方式（完全控制）

```rust
use yue::http_client::YueRequest;
use reqwest::Method;

let request = YueRequest {
    info: &request_info,
    param: Some("symbol=BTCUSDT&interval=5m&limit=10".to_string()),
    request_builder: NonAuthRequestBuilder::new(),
    body: None,
    method: Method::GET,
    response_handler: BinanceResponseHandler::new(),
    _phantom: PhantomData,
};

let result: Vec<BinanceKline> = request.perform_request_async().await?;
```

## 限流管理

### 获取限流器

```rust
use yue::binance::bn_restful_commands::{
    get_bn_spot_limit,
    get_bn_swap_limit,
    get_bn_funding_rate_limit,
};

// 现货限流：1190 请求/分钟
let spot_limiter = get_bn_spot_limit();

// 合约限流：1200 请求/分钟
let swap_limiter = get_bn_swap_limit();

// 资金费率限流：95 请求/分钟
let funding_limiter = get_bn_funding_rate_limit();
```

### 限流器配置

```rust
// yue/src/binance/bn_restful_commands.rs
static SPOT_RATE_PER_MINUTE: u32 = 1190;

define_rate_limiter!(
    SPOT_RATE_LIMITER,
    SPOT_RATE_PER_MINUTE,
    get_bn_spot_limit
);
```

### 手动检查限流

```rust
use yue::http_client::check_rate_limit;

check_rate_limit(
    2,                        // 权重
    &limiter,                 // 限流器
    2                         // 超时秒数
).await?;
```

## 错误处理

### 常见错误

```rust
use yue::errors::YueError;

match result {
    // HTTP 错误
    Err(YueError::ExchangeRequestError { code, body }) => {
        error!("API error {}: {}", code, body);
    }
    
    // 解析错误
    Err(YueError::SerdeError(e)) => {
        error!("JSON parse error: {}", e);
    }
    
    // 限流超时
    Err(YueError::CustomError(msg)) if msg.contains("限流") => {
        error!("Rate limit timeout: {}", msg);
    }
    
    // 网络错误
    Err(YueError::RequestError(e)) => {
        error!("Network error: {}", e);
    }
    
    Ok(data) => println!("Success"),
}
```

## 参数序列化

### ToQueryParams Trait

```rust
use yue::binance::bn_models::common::ToQueryParams;
use std::collections::BTreeMap;

impl ToQueryParams for BTreeMap<&str, String> {
    fn to_query_string(&self) -> String {
        self.iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&")
    }
}

// 使用
let mut params = BTreeMap::new();
params.insert("symbol", "BTCUSDT".to_string());
params.insert("interval", "1h".to_string());
let query_string = params.to_query_string();  // "interval=1h&symbol=BTCUSDT"
```

### 自定义参数结构

```rust
use yue::binance::bn_models::common::ToQueryParams;

#[derive(Debug)]
pub struct KlineParam {
    pub symbol: String,
    pub interval: String,
    pub limit: u32,
}

impl ToQueryParams for KlineParam {
    fn to_query_string(&self) -> String {
        format!("symbol={}&interval={}&limit={}",
                self.symbol,
                self.interval,
                self.limit)
    }
}
```

## 完整示例

```rust
use yue::http_client::{init_http_client, NonAuthRequestBuilder};
use yue::binance::bn_restful_commands::*;
use yue::binance::bn_models::common::ServerTime;
use yue::binance::bn_models::spot_restful::Ticker24hr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化
    init_http_client(None);
    
    let builder = NonAuthRequestBuilder::new();
    
    // 获取服务器时间
    let server_time = execute_bn_get::<
        EmptyQueryParams,
        NonAuthRequestBuilder,
        ServerTime
    >(
        &SERVER_TIME_COMMAND,
        None,
        builder.clone(),
    )
    .execute()
    .await?;
    
    println!("Server time: {}", server_time.time);
    
    // 获取24小时行情
    let mut params = std::collections::BTreeMap::new();
    params.insert("symbol", "BTCUSDT".to_string());
    
    let ticker = execute_bn_get::<
        std::collections::BTreeMap<&str, String>,
        NonAuthRequestBuilder,
        Ticker24hr
    >(
        &SPOT_TICKER_24HR_ONE_SYMBOL_COMMAND,
        Some(&params),
        builder.clone(),
    )
    .execute()
    .await?;
    
    println!("BTCUSDT price: {}", ticker.last_price);
    
    Ok(())
}
```

## 重试机制

### 使用 backon 库

```rust
use backon::{Backoff, Retryable};

let result = (async {
    execute_bn_get(&command, None, builder).execute().await
})
.retry(
    Backoff::builder()
        .with_max_retries(3)
        .with_base(std::time::Duration::from_millis(100))
        .build_exponential()
)
.await?;
```
