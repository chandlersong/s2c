mod settings;
mod database;
mod binance;
mod robots;

use crate::binance::start_binance_job;
use log::{info, LevelFilter};
use maester::tools::logs::setup_logger;
use std::time::Duration;
use tokio::signal;
use tokio::time::sleep;

#[tokio::main(flavor = "multi_thread", worker_threads = 100)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    let _ = setup_logger(Some(LevelFilter::Debug));
    
    start_binance_job().await;
    // 启动后台任务
    tokio::spawn(async move {
        let mut days = 1;
        loop {
            tokio::select! {
                _ = signal::ctrl_c() => {
                    info!("server shutdown");
                }
                _ = sleep(Duration::from_secs(24 * 60 * 60))=>{
                    info!("Running {} days at {}", days, chrono::Local::now());
                }
            }
            days = days + 1;
        }
    });
    signal::ctrl_c().await?;
    Ok(())
}
