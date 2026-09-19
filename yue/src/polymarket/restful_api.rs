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
    let base_info: &RequestInfo = &SERIES_BY_ID_COMMAND;
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
    let base_info: &RequestInfo = &EVENT_BY_ID_COMMAND;
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
    let base_info: &RequestInfo = &MARKET_BY_ID_COMMAND;
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

///
/// 这样做的主要原因还是在于polymarket的历史查询。
/// 如果end_ts不加。反而会返回全部。加了，反而会报错
///
/// 业务逻辑：按注释语义实现“按时间窗口拉取价格历史”，并在返回前做收尾过滤。
/// 1. 构造 URL 并请求 PRICES_HISTORY。
/// 2. 若返回的历史点中存在 t > end_ts，则过滤掉所有 t > end_ts 的点。
/// 3. 若 end_ts 不为 null 且返回点中最大的 t 仍然小于 end_ts，并且剩余窗口大于当前 interval，
///    则继续按最后一个点的时间再请求一次，直到没有新增结果或窗口已经非常小。
/// 4. 若 end_ts 为 null，则直接返回原始结果。
///
pub async fn query_prices_history(query: GetPricesHistoryQuery) -> Result<GetPricesHistoryResponse, YueError> {
    let client = HTTP_CLIENT.get().ok_or(YueError::new("HTTP 客户端没有初始化"))?;
    let base_info: &RequestInfo = &PRICES_HISTORY_COMMAND;
    let mut current_query = query.clone();

    let Some(end_ts) = query.end_ts else {
        let query_string = current_query.to_query_string();
        let url = format!("{}?{}", base_info.as_ref().as_str(), query_string);

        let req_info = RequestInfo::new_full_url(url, base_info.host.clone(), base_info.has_security, base_info.weight, None, None)
            .map_err(|e| YueError::new(&format!("构造请求信息失败: {}", e)))?;

        let rb = client.get(req_info.as_ref().as_str());
        let resp = execute_public_json_request::<GetPricesHistoryResponse>(&req_info, rb).await?;
        return Ok(resp);
    };

    let interval_seconds = current_query.interval.as_ref().map(|interval| interval.to_second()).unwrap_or(0);
    let mut merged_history: Vec<crate::polymarket::restful_models::MarketPriceHistoryPoint> = Vec::new();
    let mut last_max_t = None;

    loop {
        let query_string = current_query.to_query_string();
        let url = format!("{}?{}", base_info.as_ref().as_str(), query_string);

        let req_info = RequestInfo::new_full_url(url, base_info.host.clone(), base_info.has_security, base_info.weight, None, None)
            .map_err(|e| YueError::new(&format!("构造请求信息失败: {}", e)))?;

        let rb = client.get(req_info.as_ref().as_str());
        let mut resp = execute_public_json_request::<GetPricesHistoryResponse>(&req_info, rb).await?;
        if resp.history.is_empty() {
            break;
        }

        resp.history.retain(|point| point.t <= end_ts);
        if resp.history.is_empty() {
            break;
        }

        let current_max_t = resp.history.iter().map(|point| point.t).max().unwrap_or(0);
        if let Some(prev_max_t) = last_max_t {
            if current_max_t <= prev_max_t {
                break;
            }
        }

        for point in resp.history {
            if !merged_history.iter().any(|old_point| old_point.t == point.t) {
                merged_history.push(point);
            }
        }
        last_max_t = Some(current_max_t);

        if current_max_t >= end_ts {
            break;
        }

        let remaining_window = end_ts.saturating_sub(current_max_t);
        if interval_seconds == 0 || remaining_window <= interval_seconds {
            break;
        }

        current_query.start_ts = Some(current_max_t);
    }

    merged_history.sort_by_key(|point| point.t);
    Ok(GetPricesHistoryResponse { history: merged_history })
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

pub type PolymarketApi = Arc<dyn PolymarketApiTrait>;

pub(crate) static SHARE_POLYMARKET_API: OnceLock<Arc<PolymarketClientImpl>> = OnceLock::new();
pub fn default_polymarket_api() -> PolymarketApi {
    SHARE_POLYMARKET_API.get_or_init(|| Arc::new(PolymarketClientImpl)).clone()
}
