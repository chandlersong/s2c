use crate::settings::VARYS_CONFIG;
use log::info;
use maester::database::rolling_kv_db::{BatchData, RollingKVDB, RollingKVDBConfiguration};
use maester::notification::telegrams::OpsBot;
use maester::tools::time::get_next_utc_day_begin;
use std::path::PathBuf;
use std::time::Duration;
use tokio::signal;
use tokio::sync::{mpsc, OnceCell};
use tokio::time::sleep_until;

///
/// 这里会设计有两类数据库。
/// 1. 一个数据库负责写。
///     - 负责写的数据，通过channel把要写的写入
/// 2，其余数据库负责读

static DB_WRITER_TX: OnceCell<mpsc::Sender<BatchData>> = OnceCell::const_new();
static ONE_DAY_SECONDS: u32 = 24 * 60 * 60;



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
        let mut start = get_next_utc_day_begin() + Duration::from_secs(5);

        loop {
            tokio::select! {
                 _ = signal::ctrl_c() => {
                        info!("database analysis");
                    }
                _ = sleep_until(start) => {
                        let number = report.lock().unwrap().record_count();
                        let robot = OpsBot::new("ABC",123);
                        let message = format!("过去一天，平均每秒存入{}条数据",number/ONE_DAY_SECONDS);
                        robot.send(&message).await;
                    }
                }
            start = start + Duration::from_secs(5);
        }
    });
    persist_tx
}




