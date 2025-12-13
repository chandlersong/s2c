use yue::websockets::WebSocketClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // 示例1: 使用环境变量中的代理
    println!("示例1: 从环境变量读取代理设置");
    let _client1 = WebSocketClient::new_with_env_proxy("wss://stream.binance.com:9443/ws/btcusdt@ticker");

    // 示例2: 显式设置代理
    println!("\n示例2: 显式设置代理");
    let _client2 = WebSocketClient::new("wss://stream.binance.com:9443/ws/btcusdt@ticker").with_proxy("http://127.0.0.1:7890");

    // 示例3: 不使用代理（直连）
    println!("\n示例3: 直连（无代理）");
    let client3 = WebSocketClient::new("wss://stream.binance.com:9443/ws/btcusdt@ticker");

    // 运行其中一个客户端
    // 取消下面的注释来测试不同的连接方式

    // client1.connect_and_run().await?;
    // client2.connect_and_run().await?;
    // client3.connect_and_run(;

    Ok(())
}
