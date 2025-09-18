use crate::binance::jobs::start_bn_jobs;
use crate::errors::MingLuanError;
use actix::System;
use li::tools::logs::setup_logger;
use log::{LevelFilter, error, info};
use yue::http_client::init_http_client;

pub mod actix_jobs;
pub(crate) mod binance;
pub(crate) mod duck_db;
mod errors;
mod exchange;
#[cfg(test)]
pub mod test_utils;
pub mod utils;

#[actix::main]
async fn main() -> Result<(), MingLuanError> {
    setup_logger(Some(LevelFilter::Info)).unwrap();
    //TODO： 是否用代理进入Config
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy);

    match start_bn_jobs() {
        Ok(_) => info!("Binance jobs started successfully"),
        Err(e) => {
            error!("Failed to start Binance jobs: {}", e);
            panic!("stop process");
        }
    }
    actix_rt::signal::ctrl_c().await?;
    println!("Received Ctrl+C, shutting down...");
    System::current().stop(); // 优雅停止
    Ok(())
}
