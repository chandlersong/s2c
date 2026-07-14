use crate::errors::YueError;
use crate::http_client::{ToRequestBuilder, execute_public_json_request, get_http_client};
use crate::models::RequestInfo;
use crate::okx::models::common::{CandleResponse, InstrumentInfo, OkxListResponse};
use crate::okx::restful_constants::{HISTORY_CANDLES_COMMAND, PUBLIC_INSTRUMENTS_COMMAND};
use async_trait::async_trait;
use reqwest::RequestBuilder;
use std::sync::{Arc, OnceLock};

#[cfg_attr(feature = "mockable", mockall::automock)]
#[async_trait]
pub trait OKXApiTrait: Send + Sync {
    async fn list_instruments(&self, params: InstrumentsParam) -> Result<OkxListResponse<InstrumentInfo>, YueError>;
    async fn query_history_candle(&self, params: HistoryParams) -> Result<CandleResponse, YueError>;
}

pub struct OKXApiImpl;

#[async_trait]
impl OKXApiTrait for OKXApiImpl {
    async fn list_instruments(&self, params: InstrumentsParam) -> Result<OkxListResponse<InstrumentInfo>, YueError> {
        execute_public_json_request::<OkxListResponse<InstrumentInfo>>(
            &PUBLIC_INSTRUMENTS_COMMAND,
            params.to_request_builder(&PUBLIC_INSTRUMENTS_COMMAND),
        )
        .await
    }
    async fn query_history_candle(&self, params: HistoryParams) -> Result<CandleResponse, YueError> {
        execute_public_json_request::<CandleResponse>(&HISTORY_CANDLES_COMMAND, params.to_request_builder(&HISTORY_CANDLES_COMMAND)).await
    }
}

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

pub struct InstrumentsParam {
    inst_type: String,
    series_id: Option<String>,
    inst_family: Option<String>,
    inst_id: Option<String>,
}

impl InstrumentsParam {
    pub fn query_option(inst_family: &str) -> Self {
        Self {
            inst_type: "OPTION".to_string(),
            series_id: None,
            inst_family: Some(inst_family.to_string()),
            inst_id: None,
        }
    }
}

impl ToRequestBuilder for InstrumentsParam {
    fn to_request_builder(&self, request_info: &RequestInfo) -> RequestBuilder {
        let client = get_http_client();
        let res = client.get(request_info.as_ref().as_str());
        let mut params = vec![];
        params.push(("instType", self.inst_type.clone()));
        if let Some(series_id) = self.series_id.as_ref() {
            params.push(("seriesId", series_id.clone()));
        }
        if let Some(inst_family) = self.inst_family.as_ref() {
            params.push(("instFamily", inst_family.clone()));
        }
        if let Some(inst_id) = self.inst_id.as_ref() {
            params.push(("instId", inst_id.clone()));
        }
        res.query(&params)
    }
}

pub type OKxApi = Arc<dyn OKXApiTrait>;

pub static SHARE_OKX_API: OnceLock<Arc<OKXApiImpl>> = OnceLock::new();
pub fn default_okx_api() -> OKxApi {
    SHARE_OKX_API.get_or_init(|| Arc::new(OKXApiImpl)).clone()
}

pub async fn list_okx_option(inst_family: &str) -> Result<OkxListResponse<InstrumentInfo>, YueError> {
    // 构造 URL：替换 {id} 并添加 include_chat 参数
    // 获取 base RequestInfo 引用以读取配置
    let param = InstrumentsParam::query_option(inst_family);
    default_okx_api().list_instruments(param).await
}
