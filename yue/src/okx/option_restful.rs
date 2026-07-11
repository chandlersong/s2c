use crate::errors::YueError;
use crate::http_client::{HTTP_CLIENT, execute_public_json_request};
use crate::models::RequestInfo;
use crate::okx::models::common::{InstrumentInfo, OkxListResponse};
use crate::okx::restful_common::PUBLIC_INSTRUMENTS_COMMAND;

pub async fn list_okx_option(inst_family: &str) -> Result<OkxListResponse<InstrumentInfo>, YueError> {
    let client = HTTP_CLIENT.get().ok_or(YueError::new("HTTP 客户端没有初始化"))?;
    // 获取 base RequestInfo 引用以读取配置
    let base_info: &RequestInfo = &PUBLIC_INSTRUMENTS_COMMAND;
    let base = base_info.as_ref().as_str();

    // 构造 URL 并发起请求
    let url = format!("{}?instType=OPTION&instFamily={}", base, inst_family);

    let req_info = RequestInfo::new_full_url(url, base_info.host.clone(), base_info.has_security, base_info.weight, None, None)
        .map_err(|e| YueError::new(&format!("构造请求信息失败: {}", e)))?;

    let rb = client.get(req_info.as_ref().as_str());
    let resp = execute_public_json_request::<OkxListResponse<InstrumentInfo>>(&req_info, rb).await?;
    Ok(resp)
}
