use li::tools::logs::setup_logger;
use li::tools::time::unix_2_readable;
use log::{LevelFilter, info};
use std::collections::HashMap;
use yue::http_client::init_http_client;
use yue::okx::option_restful::list_okx_option;

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

    Ok(())
}
