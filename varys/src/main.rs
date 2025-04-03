mod settings;
mod database;
mod binance;
mod robots;

use crate::binance::start_binance_job;
use log::{info, LevelFilter};
use maester::async_endless;
use maester::tools::endless::endless_stop_tx;
use maester::tools::logs::setup_logger;
use std::time::Duration;
use tokio::signal;
use tokio::time::Instant;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    let _ = setup_logger(Some(LevelFilter::Debug));
    
    start_binance_job().await;
    let start_time = Instant::now();
    let _ = async_endless! {
            Instant::now() + Duration::from_millis(10),
            Duration::from_millis(24 * 60 * 60),
            async {
                let elapsed = start_time.elapsed();
                info!("Running {} days at {}", elapsed.as_secs()/(60*60*24), chrono::Local::now());
            },
            async {
                 info!("server shutdown");
            }
        };
    signal::ctrl_c().await?;
    Ok(())
}
