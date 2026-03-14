# BinanceRestfulClient 整合——任务文档

## 新建和改动的文件及目录

### 改动文件

| 文件 | cargo | 改动类型 |
|------|-------|---------|
| `yue/src/binance/http_client.rs` | yue | 重写：BinanceRestfulClient 新实现 |
| `yue/src/binance/bn_restful_commands.rs` | yue | 删减：删除 BNSecurityRequestBuilder、BinanceResponseHandler、execute_bn_* |
| `yue/src/http_client.rs` | yue | 精简：删除 YueRequest、ResponseHandler、YueRequestBuilder、NonAuthRequestBuilder |
| `yue/src/binance/mod.rs` | yue | 修改：`mod http_client` → `pub mod http_client` |
| `yue/src/binance/history_data.rs` | yue | 适配：调用方式改为 BinanceRestfulClient |
| `yue/src/binance/order_book.rs` | yue | 适配：调用方式改为 BinanceRestfulClient |
| `yue/src/binance/listen_key_client.rs` | yue | 适配：调用方式改为 BinanceRestfulClient |
| `yu/src/binance/bn_mcp.rs` | yu | 适配：调用方式改为 BinanceRestfulClient |
| `yue/examples/bn_restful_examples.rs` | yue | 适配：调用方式改为 BinanceRestfulClient |

### 不变的文件

| 文件 | 说明 |
|------|------|
| `yue/src/models.rs` | `RequestInfo` 结构体、`Decimal` 等不变 |
| `yue/src/tools.rs` | `sign_hmac` 函数不变 |
| `yue/src/errors.rs` | `YueError` 不变 |
| `yue/src/binance/bn_models/` | 所有 VO/model 定义不变 |

## 主要任务和里程碑

### 任务1：重写 BinanceRestfulClient（`yue/src/binance/http_client.rs`）

- [ ] 任务1.1：定义 `BinanceAuth` 内部结构体（api_key、api_secret）
- [ ] 任务1.2：重写 `BinanceRestfulClient` 结构体（移除自持 Client、RateLimiter、weight_limit，改为 `auth: Option<BinanceAuth>`）
- [ ] 任务1.3：实现 `public()` 和 `signed(api_key, api_secret)` 构造方法
- [ ] 任务1.4：实现 `build_request()` 内部方法
  - auth=None 时：复用全局 HTTP_CLIENT，直接拼 URL + query（原 NonAuthRequestBuilder 逻辑）
  - auth=Some 时：HMAC 签名 + X-MBX-APIKEY header（原 BNSecurityRequestBuilder 逻辑）
- [ ] 任务1.5：实现 `execute()` 内部方法（核心执行循环）
  - 限流检查：使用 `check_rate_limit(info.weight, info.rate_limit, info.get_rate_limit_timeout())`
  - 429/418 重试：解析 retry-after header，指数退避 + jitter
  - x-mbx-used-weight-1m 监控：日志告警
  - 成功响应：`serde_json::from_slice::<U>` 反序列化
  - 非 2xx 非 429/418：返回 `ExchangeRequestError`
  - 网络瞬时错误（timeout/connect/request）：指数退避重试
- [ ] 任务1.6：实现 `get()`、`post()`、`put()` 公开方法，委托到 `execute()`
- [ ] 任务1.7：实现 `retryable_get()` 方法，返回闭包兼容 backon `.retry()`
- [ ] 任务1.8：将 `yue/src/binance/mod.rs` 中 `mod http_client` 改为 `pub mod http_client`

### 任务2：迁移 bn_restful_commands.rs

- [ ] 任务2.1：删除 `BNSecurityRequestBuilder` 结构体及其 `YueRequestBuilder` 实现
- [ ] 任务2.2：删除 `BinanceResponseHandler` 结构体及其 `ResponseHandler` 实现
- [ ] 任务2.3：删除 `execute_bn_get` 函数
- [ ] 任务2.4：删除 `execute_bn_post` 函数
- [ ] 任务2.5：删除 `execute_bn_put` 函数
- [ ] 任务2.6：确保所有 `LazyLock<RequestInfo>` 命令定义、`define_rate_limiter!` 宏、URL/PATH 常量保持不变
- [ ] 任务2.7：清理不再需要的 import（`YueRequest`、`YueRequestBuilder`、`ResponseHandler` 等）

### 任务3：迁移 yue 内部调用方

- [ ] 任务3.1：迁移 `history_data.rs`
  - `execute_ping()` → `BinanceRestfulClient::public().get()`
  - `get_trading_spot_symbols()` → `BinanceRestfulClient::public().get()`
  - `get_trading_swap_symbols()` → `BinanceRestfulClient::public().get()`
  - `SimpleHistoryFetcher::get_all_kline_data()` → `BinanceRestfulClient::public().retryable_get().retry()`
  - 清理 import：移除 `execute_bn_get`、`NonAuthRequestBuilder`
- [ ] 任务3.2：迁移 `order_book.rs`
  - `InitActor` 中的 depth 请求 → `BinanceRestfulClient::public().get()`
  - 清理 import：移除 `execute_bn_get`、`NonAuthRequestBuilder`
- [ ] 任务3.3：迁移 `listen_key_client.rs`
  - `fetch_new_listen_key()` → `BinanceRestfulClient::signed().post()`
  - `renew_current_listen_key()` → `BinanceRestfulClient::signed().put()`
  - 清理 import：移除 `execute_bn_post`、`execute_bn_put`、`BNSecurityRequestBuilder`

### 任务4：迁移 yu 层调用方

- [ ] 任务4.1：迁移 `yu/src/binance/bn_mcp.rs`
  - `price_change_24h()` → `BinanceRestfulClient::public().get()`
  - 清理 import：移除 `execute_bn_get`、`NonAuthRequestBuilder`

### 任务5：迁移 examples

- [ ] 任务5.1：迁移 `yue/examples/bn_restful_examples.rs`
  - 所有 `execute_bn_get` 调用 → `BinanceRestfulClient::public().get()`
  - 不再需要显式传 `NonAuthRequestBuilder {}`
  - 清理 import

### 任务6：精简 yue/src/http_client.rs

- [ ] 任务6.1：删除 `YueRequest` 结构体及其所有方法（`execute`、`into_retryable`、`retry`、`perform_request_async`）
- [ ] 任务6.2：删除 `ResponseHandler` trait
- [ ] 任务6.3：删除 `YueRequestBuilder` trait
- [ ] 任务6.4：删除 `NonAuthRequestBuilder` 结构体及其 impl
- [ ] 任务6.5：保留 `HTTP_CLIENT`、`init_http_client()`、`DefaultRateLimiter`、`check_rate_limit()`、`ClonableResponseCache`
- [ ] 任务6.6：清理不再需要的 import（`PhantomData`、`Backoff`、`async_trait` 等）

### 任务7：更新测试

- [ ] 任务7.1：重写 `bn_restful_commands.rs` 中的测试
  - `test_compose_request_with_valid_security_info` → 测试 `BinanceRestfulClient::signed().build_request()`（签名结果不变）
  - `test_compose_request_without_security_info` → 测试 `BinanceRestfulClient::public().build_request()`
  - `test_compose_request_with_query_params` → 测试带参数的签名请求
  - `test_execute_bn_get_basic` → `BinanceRestfulClient::public().get()` wiremock 测试
  - `test_execute_bn_get_with_params` → 带参数 wiremock 测试
  - `test_execute_bn_get_with_security` → `BinanceRestfulClient::signed().get()` wiremock 测试
  - `test_binance_response_handler_*` → 在 `BinanceRestfulClient` 层面测试响应处理（成功、错误、header 解析）
- [ ] 任务7.2：确保 `history_data.rs` 中的测试通过
  - `test_get_all_kline_data_normal_case`
  - `test_get_all_kline_data_pagination`
  - `test_get_all_kline_data_api_error`
  - `test_get_all_kline_data_exactly_1000`
  - `test_get_all_kline_data_discard_non_closed_kline`
  - `test_get_all_kline_data_not_exceed_end_time`
- [ ] 任务7.3：运行全量 `cargo test` 确保无回归
- [ ] 任务7.4：运行 `cargo clippy` 确保无 warning

