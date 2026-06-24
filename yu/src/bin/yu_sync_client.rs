use std::error::Error;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use yu::sync::sync_server::grpc_sync::client_message::Payload;
use yu::sync::sync_server::grpc_sync::sync_server_client::SyncServerClient;
use yu::sync::sync_server::grpc_sync::{ClientMessage, Initial, ServerMessage};

pub mod grpc_sync {
    tonic::include_proto!("grpc_sync");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 连接服务器
    let mut client = SyncServerClient::connect("http://[::1]:50051").await?;
    println!("已连接到 gRPC 服务端");

    // 创建用于发送消息的 channel
    let (tx, rx) = mpsc::channel::<ClientMessage>(32);
    let outbound = ReceiverStream::new(rx);

    // 启动双向流
    let response = client.sync(outbound).await?;
    let mut inbound = response.into_inner(); // 服务端返回的流

    // 后台任务：持续发送消息给服务端
    let sender_task = tokio::spawn(async move {
        let msg = ClientMessage {
            payload: Some(Payload::Initial(Initial { local_max_timestamp: 0 })),
        };

        if tx.send(msg).await.is_err() {
            println!("发送通道已关闭");
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    });

    // 主任务：接收服务端消息（重点处理 oneof）
    println!("开始接收服务端消息...\n");

    while let Some(result) = inbound.next().await {
        match result {
            Ok(msg) => handle_server_message(msg),
            Err(e) => {
                eprintln!("接收错误: {}", e);
                break;
            }
        }
    }

    // 等待发送任务结束
    let _ = sender_task.await;
    println!("客户端退出");

    Ok(())
}

// 处理服务端 oneof 消息
fn handle_server_message(msg: ServerMessage) {
    if let Some(payload) = msg.payload {
        match payload {
            yu::sync::sync_server::grpc_sync::server_message::Payload::PolymarketHistory(history) => {
                println!("收到 PolymarketHistory 消息: timestamp = {}", history.timestamp);
                for (i, item) in history.history_list.iter().enumerate() {
                    println!("  历史记录 {}: {:?}", i + 1, item);
                }
            }
        }
    } else {
        println!("[未知消息类型]");
    }
}
