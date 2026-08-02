use li::tools::logs::{parse_level, setup_logger};
use log::{LevelFilter, debug, error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast::Sender;
use tokio::sync::{RwLock, broadcast};
use tonic::transport::Server;
use yu::config::get_config;
use yu::cron_job;
use yu::duck_db::DuckDBDSProvider;
use yu::errors::YuError;
use yu::polymarket::database::initial_tables;
use yu::polymarket::po::PolyMarketInstrumentPo;
use yu::polymarket::service::new_series_history_market_service;
use yu::sync::models::grpc_sync::PolyMarketHistory;
use yu::sync::models::grpc_sync::sync_interface_server::SyncInterfaceServer;
use yu::sync::server::sync_server::{YuSyncServer, get_asset_timestamp};
use yue::http_client::init_http_client;
use yue::models::HistoryInterval;
use yue::polymarket::restful_api::default_polymarket_api;

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
    if let Err(e) = initial_tables(None) {
        error!("Error initial tables: {}", e);
        return Err(e);
    }
    let log_in_config = app_config.log_level.as_deref();
    special_log.insert("yu_sync_server".to_string(), parse_level(log_in_config));
    special_log.insert("yu".to_string(), parse_level(log_in_config));
    special_log.insert("yue".to_string(), parse_level(log_in_config));
    special_log.insert("li".to_string(), parse_level(log_in_config));
    setup_logger(Some(LevelFilter::Warn), special_log)?;

    let asset_timestamp = get_asset_timestamp(DuckDBDSProvider::default()).await;
    let (polymarket_history_tx, _) = broadcast::channel(100000);
    let asset_infos = Arc::new(RwLock::new(vec![]));
    let server = YuSyncServer::new(
        polymarket_history_tx.clone(),
        asset_timestamp.clone(),
        None,
        asset_infos.clone(),
        sync_server_config.get_batch_size(),
    )
    .await;
    let series_ids = match sync_server_config.series_ids {
        None => {
            error!("No sync server series_ids provided");
            return Err(YuError::new("sync_server的series id没有找到"));
        }
        Some(ref ids) => {
            if ids.is_empty() {
                error!("sync server series_ids is empty");
                return Err(YuError::new("sync_server的series id没有找到"));
            }
            for id in ids {
                info!("Sync server subscribe series id: {}", id);
            }
            ids.clone()
        }
    };
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
