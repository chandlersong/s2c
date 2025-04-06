use crate::database::get_db_write_tx;
use crate::robots::OPS_ROBOTS;
use crate::settings::VARYS_CONFIG;
use braavos::binance::bn_dashboard::get_spot_client;
use braavos::binance::bn_models::bin::{SpotDepth, Trade};
use log::info;
use maester::database::rolling_kv_db::BatchData;
use maester::endless_select;
use maester::tools::endless::endless_stop_tx;
use prost::Message;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;

pub async fn start_binance_job() {
    info!("Starting binance job");
    tokio::join!(start_trades(), 
                 start_mini_ticker(),
                 start_subscribe_depth());
    let ws_client = get_spot_client().await;
    let mut rx = ws_client.subscribe_connected().subscribe();
    endless_select!(
        _ = rx.recv() =>{
            let _ = &OPS_ROBOTS.send("websocket重新连接").await;  
        }
    );
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
    let _ = endless_select!(
                 trade_raw_data = rx.recv() => {
                     match trade_raw_data {
                        Ok(trade_raw) => {
                             let mut buf = Vec::new();
                             let key = format!("bn:trade:spot:{}:{}:{}",
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
        );
}

pub async fn start_mini_ticker() {
    if let Some(_) = &VARYS_CONFIG.binance.spot.mini_ticker {
        //先放空
        return;
    }
    info!("monitor binance spot mini ticker");
}

#[derive(Debug, Deserialize)]
pub struct DepthConfiguration {
    pub symbol: String,
    pub level: u8,
    pub frequency: u16,
}

impl fmt::Display for DepthConfiguration {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "symbol {}, level : {}, frequency: {}",
               self.symbol, self.level, self.frequency)
    }
}

pub async fn start_subscribe_depth() {
    if let Some(depth_1000_list) = &VARYS_CONFIG.binance.spot.depth_1000ms {
        info!("Starting subscribe depth");
        for depth in depth_1000_list {
            info!("Starting subscribe depth:{}", depth);
            subscribe_one_depth(&DepthConfiguration {
                symbol: depth.clone(),
                level: 20,
                frequency: 1000,
            }).await;
        }
    }

    if let Some(depth_100_list) = &VARYS_CONFIG.binance.spot.depth_100ms {
        info!("Starting subscribe depth");
        for depth in depth_100_list {
            info!("Starting subscribe depth:{}", depth);
            subscribe_one_depth(&DepthConfiguration {
                symbol: depth.clone(),
                level: 20,
                frequency: 100,
            }).await;
        }
    }
}

pub async fn subscribe_one_depth(depth_config: &DepthConfiguration) {
    let db_tx = get_db_write_tx().await;
    let mut ws_client = get_spot_client().await;
    let symbol = &depth_config.symbol;
    let mut rx = ws_client.subscribe_depth(symbol,
                                           depth_config.level,
                                           depth_config.frequency).await;
    let trade_pair = symbol.to_string();
    let _ = endless_select!(
                depth_data = rx.recv() => {
                     match depth_data {
                        Ok(depth) => {
                             let mut buf = Vec::new();
                             let key = format!("exchange:depth:tradeType:{}:{}",
                                                               &trade_pair,
                                                               depth.event_time).as_bytes().to_vec();
                              SpotDepth::from(depth).encode(&mut buf).unwrap();
                              let mut data:BatchData = HashMap::new();
                              data.insert(key, buf);
                              db_tx.send(data).await.unwrap();
                        }
                         _ => {

                        }
                    }
                }
        );
}
