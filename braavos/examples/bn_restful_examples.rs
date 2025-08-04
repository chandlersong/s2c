use braavos::binance::bn_models::ServerTime;
use braavos::binance::bn_restful_commands::{execute_bn_get, execute_ping, SERVER_TIME_COMMAND};
use braavos::binance::bn_tools::unix_2_readable;
use braavos::http_client::init_http_client;
use braavos::models::create_empty_param;

///
/// 这个example的主要作用是
/// 1. 展示如果去

#[tokio::main]
async fn main() {
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy);
    let ping_result = execute_ping().await;
    println!("ping的结果{}", ping_result.is_ok());
    let server_time: ServerTime = execute_bn_get(&SERVER_TIME_COMMAND, create_empty_param(), None).await.unwrap();
    println!("server time is {}", unix_2_readable(&server_time.time))
}
