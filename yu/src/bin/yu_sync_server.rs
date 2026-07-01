use tonic::transport::Server;
use yu::sync::sync_server::YuSyncServer;
use yu::sync::sync_server::grpc_sync::sync_interface_server::SyncInterfaceServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse().unwrap();
    println!("gRPC 双向流服务 (oneof) 已启动 → {}", addr);

    Server::builder()
        .add_service(SyncInterfaceServer::new(YuSyncServer::default()))
        .serve(addr)
        .await?;

    Ok(())
}
