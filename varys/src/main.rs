use log::{info, LevelFilter};
use maester::tools::logs::setup_logger;
use std::time::Duration;
use tokio::signal;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    let _ = setup_logger(Some(LevelFilter::Debug));
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);

    // 启动后台任务
    tokio::spawn(async move {
        loop {
            info!("Running at {}", chrono::Local::now());
            sleep(Duration::from_secs(1)).await;
            if let Ok(()) = rx.try_recv() {
                break;
            }
        }
    });

    // 等待关闭信号
    signal::ctrl_c().await?;
    info!("Received shutdown signal, sending exit...");
    tx.send(()).await.unwrap();
    Ok(())
}
