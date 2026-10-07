use li::tools::logs::{parse_level, setup_logger};
use log::{LevelFilter, error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tonic::transport::Server;
use yu::binance::bn_dashboard::BinanceDashboard;
use yu::binance::jobs::initial_tables as initial_binance_tables;
use yu::binance::sync_server::BinanceSyncInstrumentService;
use yu::config::get_config;
use yu::errors::YuError;
use yu::okx::sync_job::start_okx_option_service;
use yu::okx::sync_server::OkxSyncInstrumentService;
use yu::polymarket::database::initial_polymarket_tables;
use yu::polymarket::sync_job::start_polymarket_sync_series_job;
use yu::polymarket::sync_server::PolyMarketSyncInstrumentService;
use yu::sync::models::grpc_sync::sync_interface_server::SyncInterfaceServer;
use yu::sync::server::sync_server::YuSyncServer;
use yue::http_client::init_http_client;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let app_config = get_config();

    let sync_server_config = match &app_config.sync_server {
        None => {
            error!("No sync server config provide provided");
            return Err(YuError::new("sync_server 配置未找到，请在配置文件中添加 sync_server 配置"));
        }
        Some(config) => config,
    };
    let addr = format!("[::]:{}", sync_server_config.get_server_port()).parse().unwrap();

    let proxy = app_config.proxy_url.clone();
    if let Some(url_proxy) = proxy {
        info!("Using proxy: {}", url_proxy);
        init_http_client(Some(&url_proxy));
    } else {
        init_http_client(None);
    }
    let mut special_log = HashMap::new();
    if let Err(e) = initial_polymarket_tables(None) {
        error!("Error polymarket initial tables: {}", e);
        return Err(e);
    }
    if let Err(e) = initial_binance_tables(None) {
        error!("Error okx initial tables: {}", e);
        return Err(e);
    }
    let log_in_config = app_config.log_level.as_deref();
    special_log.insert("yu_sync_server".to_string(), parse_level(log_in_config));
    special_log.insert("yu".to_string(), parse_level(log_in_config));
    special_log.insert("yue".to_string(), parse_level(log_in_config));
    special_log.insert("li".to_string(), parse_level(log_in_config));
    setup_logger(Some(LevelFilter::Warn), special_log)?;

    let polymarket_history_service = start_polymarket_sync_series_job().await?;
    let okx_option_service = start_okx_option_service().await?;
    let binance_dashboard = Arc::new(BinanceDashboard::new(app_config.get_data_retention_hours()));
    binance_dashboard.execute().await?;

    let polymarket_instrument_service = PolyMarketSyncInstrumentService::new(polymarket_history_service.clone());
    let okx_instrument_service = OkxSyncInstrumentService::new(okx_option_service.clone());
    let binance_instrument_service = BinanceSyncInstrumentService::new(binance_dashboard);
    let instrument_services = vec![polymarket_instrument_service, okx_instrument_service, binance_instrument_service];

    let server = YuSyncServer::create_and_start(polymarket_history_service, okx_option_service, instrument_services).await?;
    info!("sync server start at  → {}", addr);
    Server::builder()
        .add_service(SyncInterfaceServer::new(server))
        .serve(addr)
        .await
        .map_err(|e| {
            error!("Error starting sync server: {}", e);
            YuError::new("rpc sync server failed to start")
        })?;
    info!("sync server stop at  → {}", addr);
    Ok(())
}
