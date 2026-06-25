use crate::binance::bn_restful_commands::execute_json_request;
use crate::errors::YueError;
use crate::http_client::HTTP_CLIENT;
use crate::models::{HostInfo, RequestInfo, create_share_rate_limiter};
use crate::polymarket::restful_models::{Event, Market, Series};
use std::sync::{Arc, LazyLock};

//[速率限制](https://docs.polymarket.com/cn/trading/overview#%E9%80%9F%E7%8E%87%E9%99%90%E5%88%B6)
//调试小了
pub static CLOB_OPEN_LIMIT: u32 = 15000;

pub const POLYMARKET_GAMMA: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "https://gamma-api.polymarket.com",
        CLOB_OPEN_LIMIT,
        create_share_rate_limiter(CLOB_OPEN_LIMIT, Some(CLOB_OPEN_LIMIT / 60)),
    ))
});

pub const SERIES_BY_ID: &str = "/series/{id}";
pub const EVEN_BY_ID: &str = "/events/{id}";
pub const MARKET_BY_ID: &str = "/markets/{id}";

pub static SERIES_BY_ID_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(POLYMARKET_GAMMA.clone(), SERIES_BY_ID, false, 1, None, None).unwrap());

pub static EVENT_BY_ID_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(POLYMARKET_GAMMA.clone(), EVEN_BY_ID, false, 1, None, None).unwrap());

pub static MARKET_BY_ID_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(POLYMARKET_GAMMA.clone(), MARKET_BY_ID, false, 1, None, None).unwrap());

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
    let series = execute_json_request::<Series>(&req_info, rb, None).await?;
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
    let ev = execute_json_request::<Event>(&req_info, rb, None).await?;
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
    let mkt = execute_json_request::<Market>(&req_info, rb, None).await?;
    Ok(mkt)
}
