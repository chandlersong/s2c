use li::tools::logs::setup_logger;
use li::tools::time::unix_2_readable;
use log::{LevelFilter, info};
use std::collections::HashMap;
use yue::http_client::init_http_client;
use yue::okx::option_restful::list_okx_option;
use yue::okx::restful_common::{HistoryParams, query_history_candle};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy);
    let mut special_log = HashMap::new();
    special_log.insert("yue".to_string(), LevelFilter::Info);
    special_log.insert("li".to_string(), LevelFilter::Info);
    special_log.insert("okx_option_example".to_string(), LevelFilter::Info);
    setup_logger(Some(LevelFilter::Warn), special_log)?;

    let btc_option = list_okx_option("BTC-USD").await?;
    info!("find {} options for BTC-USD", btc_option.data.len());
    for data in &btc_option.data {
        let exp_time = unix_2_readable(&data.exp_time.unwrap_or(0));
        info!("find {}, exprie at {}", data.inst_id, exp_time);
    }
    let last_one = &btc_option.data[100];
    info!("try to query {} history", last_one.inst_id);

    let query_param = HistoryParams::new_only_inst_1h(last_one.inst_id.to_string());
    let history = query_history_candle(query_param).await?;
    info!("history contains {} candles", history.data.len());
    let first_candle = history.data.first().unwrap();
    let last_candle = history.data.last().unwrap();
    info!(
        "candle begin is from {} to {}",
        unix_2_readable(&first_candle[0].parse::<u64>().unwrap_or(0)),
        unix_2_readable(&last_candle[0].parse::<u64>().unwrap_or(0))
    );

    Ok(())
}
