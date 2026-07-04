use log::{LevelFilter, error, info};
use std::collections::HashMap;
use tokio_stream::StreamExt;
use tonic::Request;

pub mod grpc_sync {
    tonic::include_proto!("grpc_sync");
}

use grpc_sync::sync_interface_client::SyncInterfaceClient;
use grpc_sync::{Empty, SubscribeRequest};
use li::tools::logs::{parse_level, setup_logger};
use yu::config::get_config;
use yu::errors::YuError;
use yu::polymarket::database::initial_tables;
use yu::sync::client::database::initial_grpc_client_tables;
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
    // let sync_client_config = match &app_config.sync_client {
    //     None => {
    //         error!("No sync client config provide provided");
    //         return Err(YuError::new("sync_client 配置未找到，请在配置文件中添加 sync_client 配置"));
    //     }
    //     Some(config) => config,
    // };
    // //FUTURE:改成https
    // let server_url = format!("http://{}:{}", sync_client_config.server_host, sync_client_config.server_port);
    // info!("连接到远程服务器:{}", server_url);
    // // 连接到 gRPC 服务（根据需要修改地址）
    // let mut client = SyncInterfaceClient::connect(server_url).await?;
    // println!("已连接到 gRPC 服务端");
    //
    // // 1) 调用 GetLatestTimestamps
    // let resp = client.get_poly_market_assert_info(Request::new(Empty {})).await?;
    // let asset_ts = resp.into_inner();
    // println!("最新时间戳列表：");
    // for (asset, info) in &asset_ts.timestamps {
    //     println!("  {} => {}", asset, info.latest_timestamp);
    // }
    //
    // // 取最大的时间戳（如果需要用于后续逻辑）
    // // let max_ts = asset_ts.timestamps.values().copied().max().unwrap_or(0);
    // // println!("最大时间戳: {}", max_ts);
    //
    // // 2) 订阅 SubscribeLatest 并打印收到的所有消息
    // let mut stream = client.subscribe_latest(Request::new(SubscribeRequest {})).await?.into_inner();
    //
    // println!("开始监听 SubscribeLatest 流：");
    // while let Some(item) = stream.next().await {
    //     let msg = item?; // grpc_sync::ServerMessage
    //     if let Some(payload) = msg.payload {
    //         match payload {
    //             grpc_sync::server_message::Payload::PolymarketHistory(list) => {
    //                 println!("收到 PolyMarketHistoryList timestamp={}", list.timestamp);
    //                 for h in list.history_list {
    //                     println!("asset={} ts={} price={}", h.asset_id, h.timestamp, h.price);
    //                 }
    //             }
    //             _ => {
    //                 println!("收到其他类型 payload");
    //             }
    //         }
    //     } else {
    //         println!("收到 ServerMessage，但 payload 为空");
    //     }
    // }

    println!("订阅流结束");
    Ok(())
}
