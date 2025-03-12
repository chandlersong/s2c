use crate::robots::OPS_ROBOTS;
use crate::settings::VARYS_CONFIG;
use log::info;
use maester::database::rolling_kv_db::{BatchData, RollingKVDB, RollingKVDBConfiguration, RollingKvDBReport};
use std::path::PathBuf;
use std::time::Duration;
use tokio::signal;
use tokio::sync::{mpsc, OnceCell};
use tokio::time::{sleep_until, Instant};

///
/// 这里会设计有两类数据库。
/// 1. 一个数据库负责写。
///     - 负责写的数据，通过channel把要写的写入
/// 2，其余数据库负责读

static DB_WRITER_TX: OnceCell<mpsc::Sender<BatchData>> = OnceCell::const_new();
static ONE_HOUR_SECONDS: u32 =  60 * 60;



pub async fn get_db_write_tx() -> mpsc::Sender<BatchData> {
    DB_WRITER_TX.get_or_init(initial_db).await.clone()
}


async fn initial_db() -> mpsc::Sender<BatchData> {
    let path = PathBuf::from(&VARYS_CONFIG.db_path);
    let config = RollingKVDBConfiguration::new_with_path(path);
    info!("Starting RollingKVDB DB with {}",config);

    let (persist_tx, mut persist_rx) = mpsc::channel(1000);
    let mut db = RollingKVDB::new(config, None).await;
    let report = db.get_report().await;
    tokio::spawn(async move {
        loop {
            tokio::select! {
                 _ = signal::ctrl_c() => {
                        db.close().await;
                        info!("stop writing db");
                    }
                record = persist_rx.recv()=> {
                        if let Some(kv) = record {
                              match db.write_batch(kv){
                                    Ok(_) => {}
                                    Err(_) => {}
                              }}
                        
                    }
                }
        }
    });
    tokio::spawn(async move {
        let mut start = Instant::now() + Duration::from_secs(60);

        loop {
            let sleep_seconds = ONE_HOUR_SECONDS;
            tokio::select! {
                 _ = signal::ctrl_c() => {
                        info!("database analysis stop");
                    }
                _ = sleep_until(start) => {
                        let number = report.lock().unwrap().record_count();
                        *report.lock().unwrap() = RollingKvDBReport::default();
                        let message = format!("过去一天，平均每秒存入{}条数据",number/sleep_seconds);
                        let _ = &OPS_ROBOTS.send(&message).await;
                    }
                }
            start = start + Duration::from_secs(sleep_seconds as u64);
        }
    });
    persist_tx
}




