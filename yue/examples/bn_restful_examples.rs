use li::tools::time::unix_2_readable;
use std::collections::BTreeMap;
use yue::binance::bn_models::BinanceKline;
use yue::binance::bn_models::{EmptyQueryParams, ServerTime};
use yue::binance::bn_restful_commands::SPOT_KLINE_HISTORY_COMMAND;
use yue::binance::bn_restful_commands::{SERVER_TIME_COMMAND, execute_bn_get};
use yue::binance::history_data::execute_ping;
use yue::http_client::{NonAuthRequestBuilder, init_http_client};

///
/// 币安REST API示例 - 无需API密钥
///
/// 这个示例演示了：
/// 1. 如何初始化和配置HTTP客户端
/// 2. 如何执行基本的API调用（ping和获取服务器时间）
/// 3. 如何处理API响应
///
#[tokio::main]
async fn main() {
    // 配置本地代理（如果需要）
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy);

    // 测试API连接
    match execute_ping().await {
        Ok(_) => println!("成功连接到币安网络"),
        Err(e) => {
            println!("连接测试失败: {}", e);
            return;
        }
    }
    let request_builder = NonAuthRequestBuilder {};
    // 获取服务器时间
    match execute_bn_get::<EmptyQueryParams, NonAuthRequestBuilder, ServerTime>(&SERVER_TIME_COMMAND, None, request_builder.clone())
        .execute()
        .await
    {
        Ok(server_time) => {
            println!("测试网络服务器时间: {}", unix_2_readable(&server_time.time));
        }
        Err(e) => println!("获取服务器时间失败: {}", e),
    }

    // 获取BTCUSDT 5分钟K线范例
    let mut params = std::collections::BTreeMap::new();
    params.insert("symbol", "BTCUSDT".to_string());
    params.insert("interval", "5m".to_string());
    params.insert("limit", "5".to_string()); // 只取5根K线做演示
    match execute_bn_get::<BTreeMap<&str, String>, NonAuthRequestBuilder, Vec<BinanceKline>>(
        &SPOT_KLINE_HISTORY_COMMAND,
        Some(&params),
        request_builder.clone(),
    )
    .execute()
    .await
    {
        Ok(klines) => {
            println!("BTCUSDT 5分钟K线数据:");
            for (i, kline) in klines.iter().enumerate() {
                println!(
                    "第{}根: 开盘时间:{} 开盘价:{} 收盘价:{} 成交量:{}",
                    i + 1,
                    kline.open_time,
                    kline.open,
                    kline.close,
                    kline.volume
                );
            }
        }
        Err(e) => println!("获取K线失败: {}", e),
    }
}
