use braavos::binance::bn_restful_commands::execute_ping;
use braavos::http_client::init_http_client;

///
/// 这个example的主要作用是
/// 1. 展示如果去

#[tokio::main]
async fn main() {
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy);
    let ping_result = execute_ping().await;
    println!("ping的结果{}", ping_result.is_ok());
    println!("错误结果{}", ping_result.err().unwrap());
}
