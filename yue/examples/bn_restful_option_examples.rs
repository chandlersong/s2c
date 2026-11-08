use li::tools::logs::setup_logger;
use li::tools::time::{unix_2_readable, unix_time_now_u64_utc};
use log::{LevelFilter, info};
use std::collections::HashMap;
use yue::binance::option_api::default_binance_option_api;
use yue::binance::restful_func::CommonRequestBuilder;
use yue::errors::YueError;
use yue::http_client::init_http_client;
use yue::models::HistoryInterval;

#[tokio::main]
async fn main() -> Result<(), YueError> {
    // 配置本地代理（如果需要）
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy);
    let mut special_log = HashMap::new();
    special_log.insert("yue".to_string(), LevelFilter::Info);
    special_log.insert("li".to_string(), LevelFilter::Info);
    special_log.insert("bn_restful_option_examples".to_string(), LevelFilter::Info);
    setup_logger(Some(LevelFilter::Warn), special_log)?;

    let binance_option_api = default_binance_option_api();

    let exchange_info = binance_option_api.get_exchange_info().await?;

    info!(
        "binance server time is:{}, request_weight_limit: {:?}",
        unix_2_readable(&exchange_info.server_time),
        exchange_info.get_request_weight_limit()
    );

    let now = unix_time_now_u64_utc();
    let duration = 30 * 24 * 60 * 60 * 1000;
    let instruments = exchange_info
        .option_symbols
        .iter()
        .filter(|s| s.underlying.starts_with("BTC"))
        .filter(|s| s.expiry_date >= now && s.expiry_date <= now + duration)
        .max_by(|a, b| a.expiry_date.cmp(&b.expiry_date));
    let Some(inst) = instruments else {
        info!("no BTC option instruments found");
        return Ok(());
    };
    info!(
        "latest BTC option instrument: symbol={}, expiry_date={}",
        inst.symbol,
        unix_2_readable(&inst.expiry_date)
    );

    let command_request = CommonRequestBuilder::new(inst.symbol.clone(), 10, HistoryInterval::OneHour);
    let klines = binance_option_api.get_kline_history(command_request).await?;
    info!("latest BTC option klines num : {:?}", klines.len());
    for kline in klines {
        let close = kline.close_price()?;
        let close_time = kline.close_time()?;
        let volume = kline.volume()?;
        info!("kline: close={}, close time={}, volume={}", close, unix_2_readable(&close_time), volume);
    }

    let market_price = binance_option_api.get_all_market_price().await?;
    info!("mark price num: {:?}", market_price.len());

    Ok(())
}
