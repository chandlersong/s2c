use li::tools::logs::{parse_level, setup_logger};
use log::{LevelFilter, debug, error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::broadcast::{Receiver, Sender};
use tonic::transport::Server;
use yu::config::get_config;
use yu::cron_job;
use yu::duck_db::DuckDBDSProvider;
use yu::errors::YuError;
use yu::polymarket::database::initial_tables;
use yu::polymarket::service::new_series_history_market_service;
use yu::sync::sync_server::grpc_sync::PolyMarketHistory;
use yu::sync::sync_server::grpc_sync::sync_interface_server::SyncInterfaceServer;
use yu::sync::sync_server::{YuSyncServer, get_asset_timestamp};
use yue::http_client::init_http_client;
use yue::models::HistoryInterval;
use yue::polymarket::restful_api::default_polymarket_api;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let addr = "[::1]:50051".parse().unwrap();

    let app_config = get_config();
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
    // let series_ids = vec!["45".to_string(), "10151".to_string(), "10041".to_string()];
    let series_ids = vec!["45".to_string()];
    let polymarket_history_tx = match start_polymarket_history(series_ids, asset_timestamp.clone()).await {
        Ok(value) => value,
        Err(error) => {
            error!("polymarket seies history启动失败！！！程序退出:{}", error);
            return Err(error);
        }
    };
    let server = YuSyncServer::new(polymarket_history_tx, asset_timestamp.clone(), None).await;
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

async fn start_polymarket_history(series_ids: Vec<String>, asset_timestamp: HashMap<String, u64>) -> Result<Sender<PolyMarketHistory>, YuError> {
    let (polymarket_history_tx, _) = broadcast::channel(1000);
    let series_history_service = new_series_history_market_service(
        series_ids,
        HistoryInterval::OneHour,
        polymarket_history_tx.clone(),
        default_polymarket_api(),
    )
    .await;
    if let Err(e) = series_history_service.initial_data(asset_timestamp.clone()).await {
        error!("Error initializing series history: {}", e);
        return Err(e);
    }

    let series_history_service_clone = series_history_service.clone();
    //每天4点，因为好像很多都是4点刷新
    let _ = cron_job!("* 10 04 * * *", move |_uuid, _locked| {
        let history_split_market_job = series_history_service_clone.clone();
        Box::pin(async move {
            info!("start refresh polymarket open market");
            match history_split_market_job.clone().refresh_open_markets().await {
                Ok(_) => {
                    info!("refresh polymarket open market");
                }
                Err(e) => {
                    error!("refresh polymarket open market: {}", e);
                }
            };
        })
    });

    //每小时刷新一次
    let _ = cron_job!("30 0 * * * *", move |_, _| {
        let refresh_history_clone = series_history_service.clone();
        info!("start polymarket fetch last one hour data");
        Box::pin(async move {
            match refresh_history_clone.clone().fetch_last_one_hour_data().await {
                Ok(_) => {
                    debug!("polymarket fetch last one hour data success");
                }
                Err(e) => {
                    error!("polymarket fetch last one hour data failed: {}", e);
                }
            };
        })
    });
    Ok(polymarket_history_tx)
}
