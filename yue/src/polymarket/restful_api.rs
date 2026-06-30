use crate::errors::YueError;
use crate::http_client::{HTTP_CLIENT, execute_public_json_request};
use crate::models::{HostInfo, RequestInfo, create_share_rate_limiter};
use crate::polymarket::restful_models::{Event, GetPricesHistoryQuery, GetPricesHistoryResponse, Market, Series};
use async_trait::async_trait;
use std::sync::{Arc, LazyLock, OnceLock};

//[速率限制](https://docs.polymarket.com/cn/trading/overview#%E9%80%9F%E7%8E%87%E9%99%90%E5%88%B6)
//调试小了
pub static CLOB_OPEN_LIMIT: u32 = 15000;
pub static CLOB_API_LIMIT: u32 = 10000; // CLOB API prices-history 限流
#[cfg(not(test))]
pub const POLYMARKET_GAMMA_HOST: &str = "https://gamma-api.polymarket.com";
#[cfg(test)]
pub const POLYMARKET_GAMMA_HOST: &str = "http://127.0.0.1:20002";
// 默认生产/运行时的 CLOB host
#[cfg(not(test))]
pub const POLYMARKET_CLOB_HOST: &str = "https://clob.polymarket.com";
// 测试时使用的 CLOB host（可以指向本地 mock server 或测试环境）
#[cfg(test)]
pub const POLYMARKET_CLOB_HOST: &str = "http://127.0.0.1:20001";

pub const POLYMARKET_GAMMA: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        POLYMARKET_GAMMA_HOST,
        CLOB_OPEN_LIMIT,
        create_share_rate_limiter(CLOB_OPEN_LIMIT, Some(CLOB_OPEN_LIMIT / 60)),
    ))
});

pub const POLYMARKET_CLOB: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        POLYMARKET_CLOB_HOST,
        CLOB_API_LIMIT,
        create_share_rate_limiter(CLOB_API_LIMIT, Some(CLOB_API_LIMIT / 60)),
    ))
});

pub const SERIES_BY_ID: &str = "/series/{id}";
pub const EVEN_BY_ID: &str = "/events/{id}";
pub const MARKET_BY_ID: &str = "/markets/{id}";
pub const PRICES_HISTORY: &str = "/prices-history";

pub static SERIES_BY_ID_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(POLYMARKET_GAMMA.clone(), SERIES_BY_ID, false, 1, None, None).unwrap());

pub static EVENT_BY_ID_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(POLYMARKET_GAMMA.clone(), EVEN_BY_ID, false, 1, None, None).unwrap());

pub static MARKET_BY_ID_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(POLYMARKET_GAMMA.clone(), MARKET_BY_ID, false, 1, None, None).unwrap());

pub static PRICES_HISTORY_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(POLYMARKET_CLOB.clone(), PRICES_HISTORY, false, 1, None, None).unwrap());

pub async fn query_series_by_id(id: &str, include_chat: Option<bool>) -> Result<Series, YueError> {
    let client = HTTP_CLIENT.get().ok_or(YueError::new("HTTP 客户端没有初始化"))?;
    // 构造 URL：替换 {id} 并添加 include_chat 参数
    // 获取 base RequestInfo 引用以读取配置
    let base_info: &RequestInfo = &*SERIES_BY_ID_COMMAND;
    let base = base_info.as_ref().as_str();
    // Url::parse 会对花括号进行 percent-encoding，路径中可能出现 "%7Bid%7D"，因此尝试多种替换形式
    let mut url = base.replace("%7Bid%7D", id);

    if include_chat.is_some() {
        let include_chat_str = include_chat.unwrap().to_string().to_lowercase();
        // 没有 query 的情况，直接添加
        if url.contains('?') {
            url.push_str(format!("&include_chat={}", include_chat_str).as_ref());
        } else {
            url.push_str(format!("?include_chat={}", include_chat_str).as_ref());
        }
    }
    // 构造 RequestInfo 并发起请求
    let req_info = RequestInfo::new_full_url(url, base_info.host.clone(), base_info.has_security, base_info.weight, None, None)
        .map_err(|e| YueError::new(&format!("构造请求信息失败: {}", e)))?;

    let rb = client.get(req_info.as_ref().as_str());
    let series = execute_public_json_request::<Series>(&req_info, rb).await?;
    Ok(series)
}

pub async fn query_event_id(id: &str, include_chat: Option<bool>, include_template: Option<bool>) -> Result<Event, YueError> {
    let client = HTTP_CLIENT.get().ok_or(YueError::new("HTTP 客户端没有初始化"))?;
    let base_info: &RequestInfo = &*EVENT_BY_ID_COMMAND;
    let base = base_info.as_ref().as_str();

    let mut url = base.replace("%7Bid%7D", id);

    if include_chat.is_some() {
        let include_chat_str = include_chat.unwrap().to_string().to_lowercase();
        if url.contains('?') {
            url.push_str(format!("&include_chat={}", include_chat_str).as_ref());
        } else {
            url.push_str(format!("?include_chat={}", include_chat_str).as_ref());
        }
    }

    if include_template.is_some() {
        let include_template_str = include_template.unwrap().to_string().to_lowercase();
        if url.contains('?') {
            url.push_str(format!("&include_template={}", include_template_str).as_ref());
        } else {
            url.push_str(format!("?include_template={}", include_template_str).as_ref());
        }
    }

    let req_info = RequestInfo::new_full_url(url, base_info.host.clone(), base_info.has_security, base_info.weight, None, None)
        .map_err(|e| YueError::new(&format!("构造请求信息失败: {}", e)))?;

    let rb = client.get(req_info.as_ref().as_str());
    let ev = execute_public_json_request::<Event>(&req_info, rb).await?;
    Ok(ev)
}

pub async fn query_market_id(id: &str, include_tag: Option<bool>) -> Result<Market, YueError> {
    let client = HTTP_CLIENT.get().ok_or(YueError::new("HTTP 客户端没有初始化"))?;
    let base_info: &RequestInfo = &*MARKET_BY_ID_COMMAND;
    let base = base_info.as_ref().as_str();

    let mut url = base.replace("%7Bid%7D", id);

    if include_tag.is_some() {
        let include_tag_str = include_tag.unwrap().to_string().to_lowercase();
        if url.contains('?') {
            url.push_str(format!("&include_tag={}", include_tag_str).as_ref());
        } else {
            url.push_str(format!("?include_tag={}", include_tag_str).as_ref());
        }
    }

    let req_info = RequestInfo::new_full_url(url, base_info.host.clone(), base_info.has_security, base_info.weight, None, None)
        .map_err(|e| YueError::new(&format!("构造请求信息失败: {}", e)))?;

    let rb = client.get(req_info.as_ref().as_str());
    // TODO：execute_json_request变成共方法
    let mkt = execute_public_json_request::<Market>(&req_info, rb).await?;
    Ok(mkt)
}

pub async fn query_prices_history(query: GetPricesHistoryQuery) -> Result<GetPricesHistoryResponse, YueError> {
    let client = HTTP_CLIENT.get().ok_or(YueError::new("HTTP 客户端没有初始化"))?;
    let base_info: &RequestInfo = &*PRICES_HISTORY_COMMAND;

    // 构造 URL，将查询参数拼接到 query string 中
    let query_string = query.to_query_string();
    let url = format!("{}?{}", base_info.as_ref().as_str(), query_string);

    let req_info = RequestInfo::new_full_url(url, base_info.host.clone(), base_info.has_security, base_info.weight, None, None)
        .map_err(|e| YueError::new(&format!("构造请求信息失败: {}", e)))?;

    let rb = client.get(req_info.as_ref().as_str());
    let resp = execute_public_json_request::<GetPricesHistoryResponse>(&req_info, rb).await?;
    Ok(resp)
}

// 为 SeriesHistoryMarketService 添加可注入的客户端抽象，便于在测试中注入 mock
#[cfg_attr(feature = "mockable", mockall::automock)]
#[async_trait]
pub trait PolymarketApiTrait: Send + Sync {
    async fn query_series_by_id(&self, id: &str, include_chat: Option<bool>) -> Result<Series, YueError>;
    async fn query_event_id(&self, id: &str, include_chat: Option<bool>, include_template: Option<bool>) -> Result<Event, YueError>;
    async fn query_prices_history(&self, query: GetPricesHistoryQuery) -> Result<GetPricesHistoryResponse, YueError>;
}

pub struct PolymarketClientImpl;

#[async_trait]
impl PolymarketApiTrait for PolymarketClientImpl {
    async fn query_series_by_id(&self, id: &str, include_chat: Option<bool>) -> Result<Series, YueError> {
        query_series_by_id(id, include_chat).await
    }
    async fn query_event_id(&self, id: &str, include_chat: Option<bool>, include_template: Option<bool>) -> Result<Event, YueError> {
        query_event_id(id, include_chat, include_template).await
    }
    async fn query_prices_history(&self, query: GetPricesHistoryQuery) -> Result<GetPricesHistoryResponse, YueError> {
        query_prices_history(query).await
    }
}

pub type PolymarketAPI = Arc<dyn PolymarketApiTrait>;

pub(crate) static SHARE_POLYMARKET_API: OnceLock<Arc<PolymarketClientImpl>> = OnceLock::new();
pub fn default_polymarket_api() -> PolymarketAPI {
    SHARE_POLYMARKET_API.get_or_init(|| Arc::new(PolymarketClientImpl)).clone()
}
