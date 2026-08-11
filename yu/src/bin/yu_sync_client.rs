use li::tools::logs::{parse_level, setup_logger};
use li::tools::time::{unix_2_readable, unix_seconds_2_readable};
use log::{LevelFilter, error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::mpsc::Sender;
use tokio_stream::StreamExt;
use tonic::Request;
use yu::config::get_config;
use yu::cron_job;
use yu::errors::YuError;
use yu::sync::client::database::initial_grpc_client_tables;
use yu::sync::client::sync_client_service::{GrpcChannelManager, SyncClientService};
use yu::sync::models::grpc_sync::sync_interface_client::SyncInterfaceClient;
use yu::sync::models::grpc_sync::{Empty, Exchange, ServerMessage, SubscribeRequest, SyncRequest};
use yue::http_client::init_http_client;
use yue::tools::get_snow_flake_id_u64;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let app_config = get_config();
    let proxy = app_config.proxy_url.clone();
    if let Some(url_proxy) = proxy {
        info!("Using proxy: {}", url_proxy);
        init_http_client(Some(&url_proxy));
    } else {
        init_http_client(None);
    }
    let mut special_log = HashMap::new();
    let log_in_config = app_config.log_level.as_deref();
    special_log.insert("yu_sync_client".to_string(), parse_level(log_in_config));
    special_log.insert("yu".to_string(), parse_level(log_in_config));
    special_log.insert("yue".to_string(), parse_level(log_in_config));
    special_log.insert("li".to_string(), parse_level(log_in_config));
    setup_logger(Some(LevelFilter::Warn), special_log)?;
    initial_grpc_client_tables(None).await?;

    let client_service = Arc::new(SyncClientService::default());

    let sync_client_config = match &app_config.sync_client {
        None => {
            error!("No sync client config provide provided");
            return Err(YuError::new("sync_client 配置未找到，请在配置文件中添加 sync_client 配置"));
        }
        Some(config) => config,
    };
    // //FUTURE:改成https
    let server_url = format!("http://{}:{}", sync_client_config.server_host, sync_client_config.server_port);
    info!("连接到远程服务器:{}", server_url);
    // // 连接到 gRPC 服务（根据需要修改地址）-
    let connection_manager = Arc::new(GrpcChannelManager::new(server_url.as_ref()));

    info!("已连接到 gRPC 服务端");
    //
    // // 1) 调用 GetLatestTimestamps

    let tx = match client_service.start_batch_insert(None).await {
        Ok(sender) => sender,
        Err(e) => {
            error!("error starting batch insert: {}", e);
            return Err(e);
        }
    };

    // let subscribe_server_tx = tx.clone();
    // let subscribe_server_connection = connection_manager.clone();
    // tokio::spawn(async move {
    //     if let Err(e) = subscribe(subscribe_server_tx, subscribe_server_connection).await {
    //         error!("Error in subscribe: {}", e);
    //     }
    // });

    let sync_server_tx = tx.clone();
    let sync_server_manager = connection_manager.clone();
    let sync_client_service = client_service.clone();
    tokio::spawn(async move {
        if let Err(e) = async_sync_server(sync_client_service, sync_server_tx, sync_server_manager).await {
            error!("Error when initial instruments with server: {}", e);
        }
    });

    let daily_sync_tx = tx.clone();
    let daily_sync_manager = connection_manager.clone();
    let daily_sync_client_service = client_service.clone();

    let _ = cron_job!("0 30 5 * * *", move |_uuid, _locked| {
        let sync_tx = daily_sync_tx.clone();
        let sync_manager = daily_sync_manager.clone();
        let sync_client_service = daily_sync_client_service.clone();
        Box::pin(async move {
            info!("start refresh binance exchange info");
            if let Err(e) = async_sync_server(sync_client_service, sync_tx, sync_manager).await {
                error!("Error when async instruments with server: {}", e);
            }
        })
    });

    signal::ctrl_c().await.expect("监听 Ctrl+C 失败");
    Ok(())
}

async fn forward_server_stream(mut stream: tonic::Streaming<ServerMessage>, local_db_tx: Sender<ServerMessage>) -> Result<(), YuError> {
    while let Some(item) = stream.next().await {
        match item {
            Ok(server_message) => {
                if let Err(e) = local_db_tx.send(server_message).await {
                    error!("error sending server message: {}", e);
                }
            }
            Err(e) => {
                error!("error receiving server message: {}", e);
            }
        }
    }

    Ok(())
}

async fn subscribe(local_db_tx: Sender<ServerMessage>, manager: Arc<GrpcChannelManager>) -> Result<(), YuError> {
    loop {
        let connection = manager.connect().await;
        let mut server = SyncInterfaceClient::new(connection);
        //FUTURE：把这个identify改成配置文件的参数
        let id = get_snow_flake_id_u64();
        info!("subscribe_latest called with assigned id: {}", id);
        let request = SubscribeRequest { client_id: id };
        let stream = server.subscribe_latest(Request::new(request)).await?.into_inner();
        if let Err(e) = forward_server_stream(stream, local_db_tx.clone()).await {
            error!("error forwarding server stream: {}", e);
            manager.reconnect().await;
        }
    }
}

///
/// 这些信息并不是全部需要长连接的。所以暂时先不考虑锻炼身体
///
async fn async_sync_server(
    client_service: Arc<SyncClientService>,
    local_db_tx: Sender<ServerMessage>,
    manager: Arc<GrpcChannelManager>,
) -> Result<(), YuError> {
    let mut server = SyncInterfaceClient::new(manager.connect().await);
    let resp = server.list_instrument(Request::new(Empty {})).await?;
    let inst_list = resp.into_inner();
    info!("获取asset列表个数.{}", inst_list.instruments.len());
    let diff_from_server = client_service.align_local_instrument(inst_list).await?;
    info!("align_local_assets done. 需要同步的asset个数:{}", diff_from_server.len());
    for (inst_id, ts) in diff_from_server {
        let stream = server
            .sync_history(Request::new(SyncRequest {
                inst_id,
                timestamp: ts,
                exchange: Exchange::Polymarket.into(),
            }))
            .await?
            .into_inner();
        forward_server_stream(stream, local_db_tx.clone()).await?;
    }

    Ok(())
}
