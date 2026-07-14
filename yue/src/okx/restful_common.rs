use crate::errors::YueError;
use crate::http_client::{ToRequestBuilder, execute_public_json_request, get_http_client};
use crate::models::{DefaultRateLimiter, HostInfo, RequestInfo};
use crate::okx::models::common::CandleResponse;
use governor::middleware::StateInformationMiddleware;
use governor::{Quota, RateLimiter};
use reqwest::RequestBuilder;
use std::num::NonZeroU32;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::RwLock;

///
/// 因为根据OKX的规则。每个api都有自己的ratelimit。
/// 这个其实和之前不一样。
/// 所以，这里用最低的来做2s，10个
///
pub static BURST_NUM: u32 = 10;
pub static SPOT_RATE_PER_MINUTE: u32 = 10;
fn create_default_rate_limiter(bucket_size: u32, burst_size: Option<u32>) -> Arc<RwLock<Arc<DefaultRateLimiter>>> {
    let real_burst_size = burst_size.unwrap_or(bucket_size);
    // let quota = Quota::with_period(NonZeroU32::new(bucket_size).unwrap()).allow_burst(NonZeroU32::new(real_burst_size).unwrap().into());
    let quota = Quota::with_period(Duration::from_secs(2))
        .unwrap()
        .allow_burst(NonZeroU32::new(real_burst_size).unwrap().into());

    let res = RateLimiter::direct(quota);
    let limiter_with_info = res.with_middleware::<StateInformationMiddleware>();
    Arc::new(RwLock::new(Arc::new(limiter_with_info)))
}

pub const OKX_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "https://openapi.okx.com",
        SPOT_RATE_PER_MINUTE,
        create_default_rate_limiter(SPOT_RATE_PER_MINUTE, Some(BURST_NUM)),
    ))
});

pub const PUBLIC_INSTRUMENTS: &str = "/api/v5/public/instruments";
pub const HISTORY_CANDLES: &str = "/api/v5/market/history-candles";

pub const CANDLES: &str = "/api/v5/market/candles";
pub static PUBLIC_INSTRUMENTS_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(OKX_BASE.clone(), PUBLIC_INSTRUMENTS, false, 1, None, None).unwrap());

pub static HISTORY_CANDLES_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(OKX_BASE.clone(), HISTORY_CANDLES, false, 1, None, None).unwrap());
pub struct HistoryParams {
    inst_id: String,
    bar: Option<String>,
    after: Option<String>,
    before: Option<String>,
    limit: Option<String>,
    adjust: Option<String>,
}

impl HistoryParams {
    pub fn new_only_inst_1h(inst_id: String) -> Self {
        Self {
            inst_id,
            bar: Some("1H".to_string()),
            after: None,
            before: None,
            limit: None,
            adjust: None,
        }
    }
}

impl ToRequestBuilder for HistoryParams {
    fn to_request_builder(&self, request_info: &RequestInfo) -> RequestBuilder {
        let client = get_http_client();
        let res = client.get(request_info.as_ref().as_str());
        let mut params = vec![];
        params.push(("instId", self.inst_id.clone()));
        if let Some(bar) = self.bar.as_ref() {
            params.push(("bar", bar.clone()));
        }
        if let Some(after) = self.after.as_ref() {
            params.push(("after", after.to_string()));
        }
        if let Some(before) = self.before.as_ref() {
            params.push(("before", before.to_string()));
        }
        if let Some(limit) = self.limit.as_ref() {
            params.push(("limit", limit.to_string()));
        }
        if let Some(adjust) = self.adjust.as_ref() {
            params.push(("adjust", adjust.to_string()));
        }
        res.query(&params)
    }
}

pub async fn query_history_candle(params: HistoryParams) -> Result<CandleResponse, YueError> {
    execute_public_json_request::<CandleResponse>(&HISTORY_CANDLES_COMMAND, params.to_request_builder(&HISTORY_CANDLES_COMMAND)).await
}
