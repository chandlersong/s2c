use crate::database::get_db_write_tx;
use crate::settings::VARYS_CONFIG;
use braavos::binance::bn_dashboard::get_spot_client;
use braavos::binance::bn_models::bin::Trade;
use log::info;
use maester::database::rolling_kv_db::BatchData;
use prost::Message;
use std::collections::HashMap;

pub async fn start_binance_job() {
    info!("Starting binance job");
    tokio::join!(start_trades(), 
                 start_mini_ticker(),
                 start_subscribe_depth());
}

pub async fn start_trades() {
    if let Some(trade_list) =  &VARYS_CONFIG.binance.spot.trade{
        info!("monitor binance spot trades count: {}", trade_list.len());
        for trade in trade_list {
            info!("start monitor spot trade:{}", trade);
            subscribe_one_trade(trade).await;
        }
    }
}

pub async fn subscribe_one_trade(symbol: &str) {
    let db_tx = get_db_write_tx().await;
    let mut ws_client = get_spot_client().await;
    let mut rx = ws_client.subscribe_trade(symbol).await;
    let trade_pair = symbol.to_string();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                trade_raw_data = rx.recv() => {
                     match trade_raw_data {
                        Ok(trade_raw) => {
                             let mut buf = Vec::new();
                             let key = format!("bn:spot:{}:{}:{}",
                                                               &trade_pair,
                                                               &trade_raw.trade_id,&trade_raw.event_time).as_bytes().to_vec();
                              Trade::from(trade_raw).encode(&mut buf).unwrap();
                              let mut data:BatchData = HashMap::new();
                              data.insert(key, buf);
                              db_tx.send(data).await.unwrap();

                        }
                         _ => {

                        }
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("stop monitor trade:{}", trade_pair);
                    break;
                }
            }
        }
    });
}

pub async fn start_mini_ticker() {
    if let Some(_) = &VARYS_CONFIG.binance.spot.mini_ticker {
        //先放空
        return;
    }
    info!("monitor binance spot mini ticker");
}

pub async fn start_subscribe_depth() {
    info!("Starting subscribe depth");
}
