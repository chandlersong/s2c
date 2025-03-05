use crate::settings::VARYS_CONFIG;
use log::info;
use maester::database::rolling_kv_db::{RollingKVDB, RollingKVDBConfiguration};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::signal;
use tokio::sync::{mpsc, OnceCell};

///
/// 这里会设计有两类数据库。
/// 1. 一个数据库负责写。
///     - 负责写的数据，通过channel把要写的写入
/// 2，其余数据库负责读

static DB_WRITER_TX: OnceCell<mpsc::Sender<Entry2Persistence>> = OnceCell::const_new();

type Entry2Persistence = HashMap<&'static [u8], &'static [u8]>;

pub async fn get_db_write_tx() -> mpsc::Sender<Entry2Persistence> {
    DB_WRITER_TX.get_or_init(initial_db).await.clone()
}


async fn initial_db() -> mpsc::Sender<Entry2Persistence> {
    let path = PathBuf::from(&VARYS_CONFIG.db_path);
    let config = RollingKVDBConfiguration::new_with_path(path);
    info!("Starting RollingKVDB DB with {}",config);
    let db = RollingKVDB::new(config, None).await;

    let (persist_tx, mut persist_rx) = mpsc::channel(1000);

    tokio::spawn(async move {
        loop {
            tokio::select! {
             _ = signal::ctrl_c() => {
                    info!("stop writing db");
                }
            data = persist_rx.recv() => {
                info!("writing db,:{:?}",data);
            }
        }
        }
    });
    persist_tx
}




