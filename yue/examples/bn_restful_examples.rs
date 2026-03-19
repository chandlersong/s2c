use li::tools::logs::setup_logger_all;
use li::tools::time::{unix_2_readable, unix_time_now_u64_utc};
use log::LevelFilter;
use yue::binance::bn_models::common::{ServerTime, ToRequestBuilder};
use yue::binance::bn_models::spot_restful::{BinanceKline, Depth, Ticker24hr};
use yue::binance::bn_restful_commands::{SERVER_TIME_COMMAND, execute_json_request};
use yue::binance::bn_restful_commands::{SPOT_DEPTH_1000_COMMAND, SPOT_KLINE_HISTORY_COMMAND, SPOT_TICKER_24HR_ONE_SYMBOL_COMMAND};
use yue::binance::history_data::{CommonRequestBuilder, execute_ping};

use yue::http_client::{get_http_client, init_http_client};
use yue::models::HistoryInterval;

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
    let _ = setup_logger_all(Some(LevelFilter::Debug));
    // 测试API连接
    match execute_ping().await {
        Ok(_) => println!("成功连接到币安网络"),
        Err(e) => {
            println!("连接测试失败: {}", e);
            return;
        }
    }
    let client = get_http_client();
    let rb = client.get(SERVER_TIME_COMMAND.as_ref().as_str());
    // 获取服务器时间
    match execute_json_request::<ServerTime>(&SERVER_TIME_COMMAND, rb, None).await {
        Ok(server_time) => {
            let server_time = server_time.time;
            let local_time = unix_time_now_u64_utc();
            let gap: i64 = server_time as i64 - local_time as i64;
            println!("测试服务器时间{},local time:{},gap is {} ms", server_time, local_time, gap);
            println!("测试网络服务器时间: {}", unix_2_readable(&server_time));
        }
        Err(e) => println!("获取服务器时间失败: {}", e),
    }

    let request_builder = CommonRequestBuilder::new("BTCUSDT".to_string(), 5, HistoryInterval::FiveMinutes);
    // 获取BTCUSDT 5分钟K线范例
    match execute_json_request::<Vec<BinanceKline>>(
        &SPOT_KLINE_HISTORY_COMMAND,
        request_builder.to_request_builder(&SPOT_KLINE_HISTORY_COMMAND),
        None,
    )
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

    let ticker_24h_param = CommonRequestBuilder::only_symbol("BTCUSDT".to_string());
    match execute_json_request::<Ticker24hr>(
        &SPOT_TICKER_24HR_ONE_SYMBOL_COMMAND,
        ticker_24h_param.to_request_builder(&SPOT_TICKER_24HR_ONE_SYMBOL_COMMAND),
        None,
    )
    .await
    {
        Ok(ticker) => {
            println!("BTCUSDT 24小时价格变动:");
            println!(
                "开盘价: {}, 现在价格: {}, 最高价: {}, 最低价: {}, 成交量: {}",
                ticker.open_price, ticker.last_price, ticker.high_price, ticker.low_price, ticker.volume
            );
        }
        Err(e) => {
            println!("获取24小时的价格失败: {}", e)
        }
    }

    let depth_param = CommonRequestBuilder::symbol_and_limit("BTCUSDT".to_string(), 1000);
    match execute_json_request::<Depth>(&SPOT_DEPTH_1000_COMMAND, depth_param.to_request_builder(&SPOT_DEPTH_1000_COMMAND), None).await {
        Ok(depth) => {
            println!("BTCUSDT 24小时价格变动:");
            println!(
                "last_update_id: {}, bids len: {}, ask len: {}",
                depth.last_update_id,
                depth.bids.len(),
                depth.asks.len()
            );
            let first_bid = depth.bids.first().unwrap();
            println!("first bid price: {}, qty:{}", first_bid.0, first_bid.1);
            let first_asks = depth.asks.first().unwrap();
            println!("first asks price: {}, qty:{}", first_asks.0, first_asks.1);
        }
        Err(e) => {
            println!("获取24小时的价格失败: {}", e)
        }
    }
}
