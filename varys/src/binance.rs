use crate::database::get_db_write_tx;
use crate::settings::VARYS_CONFIG;
use log::info;
use maester::database::rolling_kv_db::BatchData;
use std::collections::HashMap;

pub async fn start_binance_job() {
    info!("Starting binance job");
    tokio::join!(start_trades(), start_mini_ticker());
}

pub async fn start_trades() {
    let trade_list = &VARYS_CONFIG.binance.spot.trade;
    info!("monitor binance spot trades count: {}", trade_list.len());
    for trade in trade_list {
        info!("start monitor spot trade:{}", trade);
    }

    let db_tx = get_db_write_tx().await;
    let kv: BatchData = HashMap::new();
    db_tx.send(kv).await.unwrap();
}

pub async fn start_mini_ticker() {
    if !&VARYS_CONFIG.binance.spot.mini_ticker {
        return;
    }
    info!("monitor binance spot mini ticker");
}
