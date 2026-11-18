use std::error::Error;
use yue::binance::bn_restful_commands::{SPOT_EXCHANGE_COMMAND, SWAP_EXCHANGE_COMMAND, execute_json_request};
use yue::http_client::{get_http_client, init_http_client};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let start = std::time::Instant::now();
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy);

    println!("开始获取币安现货交易对信息...");
    let client = get_http_client();
    let rb = client.get(SPOT_EXCHANGE_COMMAND.as_ref().as_str());
    let spot_exchange = execute_json_request::<serde_json::Value>(&SPOT_EXCHANGE_COMMAND, rb, None)
        .await
        .expect("获取现货交易对信息失败");
    let spot_symbols = spot_exchange["symbols"].as_array().ok_or("现货交易所信息中缺少有效的 symbols 数组")?;
    println!("成功获取到 {} 个现货交易对信息", spot_symbols.len());
    serde_json::to_writer_pretty(std::fs::File::create("spot_exchange.json")?, &spot_exchange)?;
    println!("现货交易所信息已保存到 spot_exchange.json");

    // 获取swap交易对信息l
    println!("开始获取币安U本位合约交易对信息...");
    let client = get_http_client();
    let rb = client.get(SWAP_EXCHANGE_COMMAND.as_ref().as_str());
    let swap_exchange = execute_json_request::<serde_json::Value>(&SWAP_EXCHANGE_COMMAND, rb, None)
        .await
        .expect("获取U本位合约交易对信息失败");
    let swap_symbols = swap_exchange["symbols"]
        .as_array()
        .ok_or("U本位合约交易所信息中缺少有效的 symbols 数组")?;
    println!("成功获取到 {} 个U本位合约交易对信息", swap_symbols.len());
    serde_json::to_writer_pretty(std::fs::File::create("swap_exchange.json")?, &swap_exchange)?;
    println!("U本位合约交易所信息已保存到 swap_exchange.json");

    println!("运行耗时: {:?}", start.elapsed());
    Ok(())
}
