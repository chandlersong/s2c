use li::tools::logs::{parse_level, setup_logger};
use log::{LevelFilter, error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::mpsc::Sender;
use tokio_stream::StreamExt;
use tonic::Request;
use tonic::transport::Channel;
use yu::config::get_config;
use yu::errors::YuError;
use yu::sync::client::database::initial_grpc_client_tables;
use yu::sync::client::sync_client_service::SyncClientService;
use yu::sync::sync_server::grpc_sync::{Empty, ServerMessage, SubscribeRequest, SyncRequest, sync_interface_client::SyncInterfaceClient};
use yue::http_client::init_http_client;

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
    // // 连接到 gRPC 服务（根据需要修改地址）
    let client: SyncInterfaceClient<Channel> = SyncInterfaceClient::connect(server_url).await?;
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
    let sync_server_tx = tx.clone();
    let sync_server_client = client.clone();
    tokio::spawn(async move {
        if let Err(e) = async_sync_server(client_service.clone(), sync_server_tx, sync_server_client).await {
            error!("Error in async_sync_server: {}", e);
        }
    });
    let subscribe_server_tx = tx.clone();
    let subscribe_server_client = client.clone();
    tokio::spawn(async move {
        if let Err(e) = subscribe(subscribe_server_tx, subscribe_server_client).await {
            error!("Error in subscribe: {}", e);
        }
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

async fn subscribe(local_db_tx: Sender<ServerMessage>, mut server: SyncInterfaceClient<Channel>) -> Result<(), YuError> {
    let stream = server.subscribe_latest(Request::new(SubscribeRequest {})).await?.into_inner();
    forward_server_stream(stream, local_db_tx).await
}

async fn async_sync_server(
    client_service: Arc<SyncClientService>,
    local_db_tx: Sender<ServerMessage>,
    mut server: SyncInterfaceClient<Channel>,
) -> Result<(), YuError> {
    let resp = server.get_poly_market_assert_info(Request::new(Empty {})).await?;
    let asset_list = resp.into_inner();
    info!("获取asset列表个数.{}", asset_list.assets.len());
    let diff_from_server = client_service.align_local_assets(asset_list).await;
    let adjust_assets = match diff_from_server {
        Ok(diff) => {
            info!("align_local_assets done. 需要同步的asset个数:{}", diff.len());
            diff
        }
        Err(e) => {
            error!("align_local_assets fail. {}", e);
            HashMap::new()
        }
    };
    for (assert_id, ts) in adjust_assets {
        let stream = server
            .sync_history(Request::new(SyncRequest {
                asset_id: assert_id,
                timestamp: ts,
            }))
            .await?
            .into_inner();
        forward_server_stream(stream, local_db_tx.clone()).await?;
    }

    Ok(())
}
