use async_trait::async_trait;
use li::tools::logs::setup_logger;
use li::tools::time::unix_2_readable;
use li::websocket::connection::{CommandMessage, MessageHandlerTrait, ToServerMessage, WebSocketConnection};
use log::{LevelFilter, error, info};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use yu::config::get_config;
use yu::errors::YuError;
use yu::okx::duckdb_repository::get_default_kline_repo;
use yu::okx::duckdb_tables::initial_okx_tables;
use yu::okx::service::fetch_history;
use yue::binance::bn_models::spot_websocket_stream::{
    BinanceSpotWebSocketStreamResponse, BinanceSpotWebSocketStreamWrapper, DepthUpdateStreamPayload,
};
use yue::http_client::init_http_client;
use yue::models::HistoryInterval;
use yue::okx::models::websocket::{ArgBody, OkxWebsocketResponse};
use yue::okx::restful_api::default_okx_api;
use yue::okx::websocket_channel::{CommandRequest, OXK_BUSINESS_WEBSOCKET, OXK_PUBLIC_WEBSOCKET};

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let app_config = get_config();
    let proxy = app_config.proxy_url.clone();
    if let Some(url_proxy) = proxy {
        info!("Using proxy: {}", url_proxy);
        init_http_client(Some(&url_proxy));
    } else {
        init_http_client(None);
    }
    let mut special_log = HashMap::new();
    special_log.insert("li".to_string(), LevelFilter::Trace);
    special_log.insert("yu".to_string(), LevelFilter::Trace);
    special_log.insert("yue".to_string(), LevelFilter::Trace);
    special_log.insert("okx_kline_fetch_example".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Warn), special_log)?;
    initial_okx_tables(None)?;
    //到时候自己找一个
    let inst_id = "BTC-USD-260726-72000-C";
    let interval = HistoryInterval::OneHour;
    let end = interval.get_now_close_unix_ms_utc() + 1;
    let start = interval.get_now_close_unix_ms_utc() - 150 * interval.to_milliseconds();
    let okx_api = default_okx_api();
    let kline_repo = get_default_kline_repo(None);
    info!(
        "start to fetch history:{}, from {} to {}",
        inst_id,
        unix_2_readable(&start),
        unix_2_readable(&end)
    );
    let mut fist_kline_timestamp = interval.get_now_close_unix_ms_utc() + interval.to_milliseconds();
    let interval_ms = interval.to_milliseconds();
    let klines = fetch_history(inst_id.clone(), start, end, &interval, &okx_api, &kline_repo, None, Some(5)).await?;
    info!("find {} klines", klines.len());
    for kline in &klines {
        let real_gap = fist_kline_timestamp - kline.ts;
        fist_kline_timestamp = kline.ts;
        if real_gap != interval_ms {
            error!("{} gap is not correct", unix_2_readable(&kline.ts))
        }
    }
    let first_candle = klines.first().unwrap();
    let last_candle = klines.last().unwrap();

    Ok(())
}
