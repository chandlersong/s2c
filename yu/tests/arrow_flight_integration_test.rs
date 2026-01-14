// 简单的 Arrow Flight 集成测试示例
// 演示如何通过 Flight do_get 接口查询 SQL 和 Depth 数据

#[cfg(test)]
mod tests {
    use yu::arrow_flight_server::start_flight_server;

    #[tokio::test]
    #[ignore] // 这是一个集成测试示例，需要手动运行
    async fn test_flight_server_depth_query() {
        // 启动 Flight 服务器
        let addr = "127.0.0.1:50051";
        match start_flight_server(addr).await {
            Ok(shutdown_tx) => {
                println!("✓ Flight 服务器启动成功，监听 {}", addr);

                // 给服务器一点时间启动
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                // 这里可以添加 gRPC 客户端代码来测试 depth 查询
                // 例如：
                // let client = FlightServiceClient::connect("http://127.0.0.1:50051").await.unwrap();
                // let ticket = Ticket { ticket: Bytes::from("depth:symbol=BTCUSDT") };
                // let mut stream = client.do_get(Request::new(ticket)).await.unwrap();
                // while let Some(data) = stream.message().await.unwrap() {
                //     println!("Received: {:?}", data);
                // }

                // 关闭服务器
                let _ = shutdown_tx.send(());
                println!("✓ Flight 服务器已关闭");
            }
            Err(e) => {
                panic!("Failed to start flight server: {}", e);
            }
        }
    }

    #[tokio::test]
    #[ignore] // 这是一个集成测试示例，需要手动运行
    async fn test_flight_server_sql_query() {
        // 启动 Flight 服务器
        let addr = "127.0.0.1:50052";
        match start_flight_server(addr).await {
            Ok(shutdown_tx) => {
                println!("✓ Flight 服务器启动成功，监听 {}", addr);

                // 给服务器一点时间启动
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                // 这里可以添加 gRPC 客户端代码来测试 SQL 查询
                // 例如：
                // let client = FlightServiceClient::connect("http://127.0.0.1:50052").await.unwrap();
                // let ticket = Ticket { ticket: Bytes::from("SELECT COUNT(*) FROM users") };
                // let mut stream = client.do_get(Request::new(ticket)).await.unwrap();
                // while let Some(data) = stream.message().await.unwrap() {
                //     println!("Received: {:?}", data);
                // }

                // 关闭服务器
                let _ = shutdown_tx.send(());
                println!("✓ Flight 服务器已关闭");
            }
            Err(e) => {
                panic!("Failed to start flight server: {}", e);
            }
        }
    }
}
