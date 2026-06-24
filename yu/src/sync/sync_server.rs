use crate::sync::sync_server::grpc_sync::client_message::Payload as ClientPayload;
use crate::sync::sync_server::grpc_sync::server_message::Payload as ServerPayload;
use crate::sync::sync_server::grpc_sync::sync_server_server::SyncServer;
use crate::sync::sync_server::grpc_sync::{ClientMessage, PolyMarketHistoryList, ServerMessage};
use std::pin::Pin;
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tonic::{Request, Response, Status, Streaming};

pub mod grpc_sync {
    tonic::include_proto!("grpc_sync");
}

#[derive(Default)]
pub struct YuSyncServer {}

#[tonic::async_trait]
impl SyncServer for YuSyncServer {
    type syncStream = Pin<Box<dyn tokio_stream::Stream<Item = Result<ServerMessage, Status>> + Send + 'static>>;

    async fn sync(&self, request: Request<Streaming<ClientMessage>>) -> Result<Response<Self::syncStream>, Status> {
        let mut inbound = request.into_inner();
        let (tx, rx) = mpsc::channel::<Result<ServerMessage, Status>>(16);

        tokio::spawn(async move {
            while let Some(next_msg) = inbound.next().await {
                match next_msg {
                    Ok(msg) => {
                        if let Some(ClientPayload::Initial(_)) = msg.payload {
                            let reply = ServerMessage {
                                payload: Some(ServerPayload::PolymarketHistory(PolyMarketHistoryList {
                                    history_list: Vec::new(),
                                    timestamp: 123,
                                })),
                            };
                            if tx.send(Ok(reply)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(status) => {
                        let _ = tx.send(Err(status)).await;
                        break;
                    }
                }
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx)) as Self::syncStream))
    }
}
