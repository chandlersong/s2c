use li::tools::logs::setup_logger;
use li::tools::time::unix_time_now_u64_utc;
use log::{LevelFilter, info};
use std::collections::HashMap;
use tonic::Request;
use yu::config::get_config;
use yu::errors::YuError;
use yu::sync::client::database::initial_grpc_client_tables;
use yu::sync::client::grpc_manager::get_grpc_manager_from_config;
use yu::sync::client::service::polymarket::PolymarketClientExchangeService;
use yu::sync::models::grpc_sync::Empty;
use yu::sync::models::grpc_sync::instrument::Payload;
use yu::sync::models::grpc_sync::sync_interface_client::SyncInterfaceClient;

///
/// 这里的主要功能。
/// 1. 方便日后调试。
/// 2. 方便后续的skill等AI工具，能够复制类似的工具。
///
///
/// 配置一个CONFIG_PATH,用服务器端的配置文件
///
#[tokio::main]
async fn main() -> Result<(), YuError> {
    let mut special_log = HashMap::new();
    special_log.insert("li".to_string(), LevelFilter::Trace);
    special_log.insert("yu".to_string(), LevelFilter::Trace);
    special_log.insert("client_exchange_service_polymarket_example".to_string(), LevelFilter::Trace);
    special_log.insert("yue".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Warn), special_log)?;
    let _app_config = get_config();

    initial_grpc_client_tables(None).await?;
    let polymarket_instrument_service = PolymarketClientExchangeService::for_production()?;

    let connection_manager = get_grpc_manager_from_config();
    info!("开始连接到 gRPC 服务端");

    let mut server = SyncInterfaceClient::new(connection_manager.connect().await);
    let resp = server.list_instrument(Request::new(Empty {})).await?;
    let inst_list = resp.into_inner();
    info!("获取instrument列表个数.{}", inst_list.instruments.len());

    let inst_id = 641321015250846635;
    let Some((instrument_key, instrument)) = inst_list.instruments.into_iter().find(|(_, instrument)| {
        if let Some(payload) = &instrument.payload {
            return match payload {
                Payload::Polymarket(p) => {
                    if p.server_id == inst_id {
                        return true;
                    }
                    false
                }
                Payload::Okx(_) => false,
                Payload::Binance(_) => false,
            };
        }
        return false;
    }) else {
        return Err(YuError::new("no Polymarket instrument found with inst_id in server instrument list"));
    };
    info!("使用instrument {} 作为示例", instrument_key);
    info!("开始instrument初始化");
    let now = unix_time_now_u64_utc();
    polymarket_instrument_service.initial_data(instrument, now).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
    info!("和服务器对账");
    polymarket_instrument_service.reconcile_living_instrument_with_server(now).await?;
    tokio::time::sleep(tokio::time::Duration::from_mins(5)).await;
    Ok(())
}
