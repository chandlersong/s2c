use std::error::Error;
use tokio_stream::StreamExt;
use tonic::Request;

pub mod grpc_sync {
    tonic::include_proto!("grpc_sync");
}

use grpc_sync::sync_interface_client::SyncInterfaceClient;
use grpc_sync::{Empty, SubscribeRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 连接到 gRPC 服务（根据需要修改地址）
    let mut client = SyncInterfaceClient::connect("http://localhost:50051").await?;
    println!("已连接到 gRPC 服务端");

    // 1) 调用 GetLatestTimestamps
    let resp = client.get_latest_timestamps(Request::new(Empty {})).await?;
    let asset_ts = resp.into_inner();
    println!("最新时间戳列表：");
    for (asset, ts) in &asset_ts.timestamps {
        println!("  {} => {}", asset, ts);
    }

    // 取最大的时间戳（如果需要用于后续逻辑）
    let max_ts = asset_ts.timestamps.values().copied().max().unwrap_or(0);
    println!("最大时间戳: {}", max_ts);

    // 2) 订阅 SubscribeLatest 并打印收到的所有消息
    let mut stream = client.subscribe_latest(Request::new(SubscribeRequest {})).await?.into_inner();

    println!("开始监听 SubscribeLatest 流：");
    while let Some(item) = stream.next().await {
        let msg = item?; // grpc_sync::ServerMessage
        if let Some(payload) = msg.payload {
            match payload {
                grpc_sync::server_message::Payload::PolymarketHistory(list) => {
                    println!("收到 PolyMarketHistoryList timestamp={}", list.timestamp);
                    for h in list.history_list {
                        println!(
                            "  series={} event={} market={} asset={} ts={} price={}",
                            h.series_id, h.event_id, h.market_id, h.asset_id, h.timestamp, h.price
                        );
                    }
                }
                _ => {
                    println!("收到其他类型 payload");
                }
            }
        } else {
            println!("收到 ServerMessage，但 payload 为空");
        }
    }

    println!("订阅流结束");
    Ok(())
}
