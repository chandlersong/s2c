use actix::{Actor, Context};
use li::tools::logs::setup_logger;
use li::tools::time::unix_time_now_u64_utc;
use log::{info, LevelFilter};
use std::collections::HashMap;
use yu::binance::bn_dashboard::BinanceDashboard;
use yu::binance::history_task::HistoryDataTask;
use yu::config::get_config;
use yu::errors::YuError;
use yu::exchange::{CloneHistoryFetcherFactory, HistoryFetcherFactory};
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_restful_commands::SWAP_KLINE_HISTORY_COMMAND;
use yue::binance::history_data::{CommonRequestBuilder, SimpleHistoryFetcher};
use yue::errors::YueError;
use yue::http_client::init_http_client;
use yue::models::HistoryInterval;
use yue::query_message::{BatchInsert, Count};

struct PrinterActor;

impl Actor for PrinterActor {
    type Context = Context<Self>;
}

impl actix::Handler<BatchInsert<BinanceKline>> for PrinterActor {
    type Result = Result<usize, YueError>;

    fn handle(&mut self, msg: BatchInsert<BinanceKline>, _ctx: &mut Self::Context) -> Self::Result {
        let data = msg.data;
        // 统计重复的funding_time，并打印所有重复行
        for d in data.iter() {
            println!("{:?}", d);
        }
        Ok(1) // 模拟成功处理，返回插入了1条记录
    }
}

impl actix::Handler<Count> for PrinterActor {
    type Result = isize;
    fn handle(&mut self, _: Count, _ctx: &mut Self::Context) -> Self::Result {
        1
    }
}

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
    special_log.insert("mingluan".to_string(), LevelFilter::Debug);
    special_log.insert("yue".to_string(), LevelFilter::Debug);
    setup_logger(Some(LevelFilter::Warn), special_log).unwrap();

    let base_swap_kline_fetcher = SimpleHistoryFetcher::new(&SWAP_KLINE_HISTORY_COMMAND);
    let swap_kline_fetcher: CloneHistoryFetcherFactory<SimpleHistoryFetcher, CommonRequestBuilder, BinanceKline> =
        CloneHistoryFetcherFactory::new(base_swap_kline_fetcher);

    let param = CommonRequestBuilder::new("GRASSUSDT".to_string(), 1000, HistoryInterval::OneHour);
    let interval = HistoryInterval::FiveMinutes;
    let now_timestamp = unix_time_now_u64_utc();
    let start_time = interval.get_close_unix_ms(now_timestamp - 10 * 60 * 1000);
    let end_time = interval.get_close_unix_ms(now_timestamp);

    let addr = PrinterActor {}.start();
    let _ = tokio::spawn(async move {
        HistoryDataTask::<
            CloneHistoryFetcherFactory<SimpleHistoryFetcher, CommonRequestBuilder, BinanceKline>,
            CommonRequestBuilder,
            BinanceKline,
            BinanceDashboard,
        >::fetch_symbol_data(
            swap_kline_fetcher.create_fetcher(),
            param,
            start_time,
            end_time,
            "test",
            interval,
            addr.recipient(),
        )
        .await;
    });

    Ok(())
}
