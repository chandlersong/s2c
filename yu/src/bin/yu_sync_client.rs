use li::tools::logs::{parse_level, setup_logger};
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
use yu::sync::client::grpc_manager::{GrpcChannelManager, get_grpc_manager_from_config};
use yu::sync::client::sync_client_service::SyncClientService;
use yu::sync::models::grpc_sync::sync_interface_client::SyncInterfaceClient;
use yu::sync::models::grpc_sync::{Empty, ServerMessage, SubscribeRequest};
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

    let client_service = Arc::new(SyncClientService::for_production()?);
    let connection_manager = get_grpc_manager_from_config();
    info!("已连接到 gRPC 服务端");
    //
    // // 1) 调用 GetLatestTimestamps

    let tx = match client_service.start_batch_insert(None, None).await {
        Ok(sender) => sender,
        Err(e) => {
            error!("error starting batch insert: {}", e);
            return Err(e);
        }
    };

    if let Err(e) = initial_grpc_client_tables(None).await {
        error!("error starting initial grpc client tables: {}", e);
        return Err(e);
    }

    let subscribe_server_tx = tx.clone();
    let subscribe_server_connection = connection_manager.clone();
    tokio::spawn(async move {
        if let Err(e) = subscribe(subscribe_server_tx, subscribe_server_connection).await {
            error!("Error in subscribe: {}", e);
        }
    });
    let sync_server_manager = connection_manager.clone();
    let sync_client_service = client_service.clone();
    tokio::spawn(async move {
        if let Err(e) = initial_data(sync_client_service, sync_server_manager).await {
            error!("Error when initial instruments with server: {}", e);
        }
    });

    let daily_client_service = client_service.clone();
    let _ = cron_job!("0 28 */6 * * *", move |_uuid, _locked| {
        let sync_client_service = daily_client_service.clone();
        Box::pin(async move {
            info!("start refresh binance exchange info");
            if let Err(e) = sync_client_service.reconcile_living_instrument_with_server().await {
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

async fn subscribe(local_db_tx: Sender<ServerMessage>, manager: GrpcChannelManager) -> Result<(), YuError> {
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
/// FUTURE:
/// 1. 可能会有一些已经关闭的instrument。这里也要同步。但是因为这里的问题其实希望client有完整信息，所以就过了吧。
async fn initial_data(client_service: Arc<SyncClientService>, manager: GrpcChannelManager) -> Result<(), YuError> {
    let channel = manager.get_channel().await;
    let mut server = SyncInterfaceClient::new(channel.clone());
    let resp = server.list_instrument(Request::new(Empty {})).await?;
    let inst_list = resp.into_inner();
    info!("获取instrument列表个数.{}", inst_list.instruments.len());
    if let Err(e) = client_service.initial_data(inst_list).await {
        error!("error initializing client: {}", e);
    }
    info!("async_sync_server done. 历史数据异步写入，可能过会儿更新");
    Ok(())
}
