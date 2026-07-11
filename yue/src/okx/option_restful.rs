use crate::errors::YueError;
use crate::http_client::{HTTP_CLIENT, execute_public_json_request};
use crate::okx::models::common::{InstrumentInfo, OkxListResponse};
use crate::okx::restful_common::PUBLIC_INSTRUMENTS_COMMAND;

pub async fn list_okx_option(inst_family: &str) -> Result<OkxListResponse<InstrumentInfo>, YueError> {
    // 构造 URL：替换 {id} 并添加 include_chat 参数
    // 获取 base RequestInfo 引用以读取配置
    let client = HTTP_CLIENT.get().ok_or(YueError::new("HTTP 客户端没有初始化"))?;
    let mut builder = client.get(PUBLIC_INSTRUMENTS_COMMAND.as_ref().as_str());
    let mut params = vec![];
    params.push(("instType", "OPTION"));
    params.push(("instFamily", inst_family));
    builder = builder.query(&params);

    execute_public_json_request::<OkxListResponse<InstrumentInfo>>(&PUBLIC_INSTRUMENTS_COMMAND, builder).await
}
