use li::tools::logs::setup_logger_all;
use li::tools::time::{unix_2_readable, unix_time_now_u64_utc};
use log::{LevelFilter, debug, error, info};
use tokio::sync::mpsc;
use yue::binance::bn_models::common::HistoryVo;
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_restful_commands::{SPOT_KLINE_HISTORY_COMMAND, SWAP_KLINE_HISTORY_COMMAND};
use yue::binance::history_data::{CommonRequestBuilder, HistoryFetcher, SimpleHistoryFetcher};
use yue::http_client::init_http_client;
use yue::models::HistoryInterval;
use yue::query_message::QueryCommand;

fn print_kline_result<H>(klines: &Vec<H>, interval: Option<HistoryInterval>)
where
    H: HistoryVo,
{
    debug!("Fetched {} data", klines.len());
    if let Some(first) = klines.first() {
        debug!("First kline open_time = {}", unix_2_readable(&first.get_open_time()));
    }
    if let Some(last) = klines.last() {
        debug!("Last kline close_time = {}", unix_2_readable(&last.get_close_time()));
    }
    debug!("total kline fetched: {}", klines.len());

    // Interval consistency check: ensure each candle's begin (open_time) interval is equal
    if klines.len() >= 2 {
        // 计算相邻 open_time 差值数组
        let mut diffs: Vec<u64> = Vec::with_capacity(klines.len() - 1);
        for w in klines.windows(2) {
            let prev = w[0].get_open_time();
            let next = w[1].get_open_time();
            let diff = if next >= prev { next - prev } else { 0 };
            diffs.push(diff);
        }

        // 如果 caller 传入了 interval，就使用该 interval 的毫秒值作为期望间隔；
        // 否则回退为以第一个 diff 为期望间隔（向后兼容）
        let expected: u64 = if let Some(iv) = interval {
            iv.to_milliseconds()
        } else if let Some(&first_diff) = diffs.first() {
            first_diff
        } else {
            0
        };

        if expected == 0 {
            error!("Detected zero/unknown expected interval; cannot validate kline spacing");
        } else {
            let mut ok = true;
            for (i, &d) in diffs.iter().enumerate() {
                if d != expected {
                    error!("Kline interval mismatch at window {}: expected {} ms, got {} ms", i, expected, d);
                    ok = false;
                }
            }
            if ok {
                debug!("Kline intervals consistent: {} ms", expected);
            }
        }
    }
}

///
/// 这个例子，主要是是获取Kline，包括以下一些数据
/// 1. spot
/// 2. swap
/// 3. 资金费率
#[tokio::main]
async fn main() {
    // Initialize http client with default settings
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy);
    let _ = setup_logger_all(Some(LevelFilter::Debug));
    let now_ms = unix_time_now_u64_utc();
    let one_hour: u64 = 60 * 60 * 1000;
    let start_ms = now_ms - 10 * one_hour;
    let end_ms = now_ms - 10 * 1000 * 60;
    println!("Now (ms) = {}, start_time (ms) = {}", now_ms, start_ms);
    let symbol = "BTCUSDT";
    let spot_kline_fetch = SimpleHistoryFetcher::kline(&SPOT_KLINE_HISTORY_COMMAND);
    let base_param = CommonRequestBuilder::new(symbol.to_string(), 1000, HistoryInterval::FiveMinutes);
    let (spot_recipient, mut spot_rx) = mpsc::channel::<QueryCommand<BinanceKline>>(100);
    let _ = spot_kline_fetch
        .get_all_kline_data(
            base_param,
            Some(HistoryInterval::FiveMinutes),
            Some(start_ms),
            Some(end_ms),
            spot_recipient,
            false,
        )
        .await;

    let spot_collector = tokio::spawn(async move {
        let mut batch_count: usize = 0;
        let mut total_klines: usize = 0;
        while let Some(cmd) = spot_rx.recv().await {
            match cmd {
                QueryCommand::BatchInsert(payload) => {
                    let symbol = payload.symbol.clone();
                    let size = payload.data.len();
                    info!("received BatchInsert for {:?}, size={}", symbol, size);
                    if size > 0 {
                        // 打印该批次的首尾时间信息
                        print_kline_result(&payload.data, Some(HistoryInterval::FiveMinutes));
                    }
                    batch_count += 1;
                    total_klines += size;
                    // 如果需要给发送方回复，可以使用 payload.result_tx.send(...)
                    // 目前这里不回复，直接让 result_tx 被丢弃。
                }
                QueryCommand::GetCount(resp) => {
                    // 返回当前已统计的总数（示例中使用 usize -> Result<usize, YueError>）
                    let _ = resp.send(Ok(total_klines));
                }
            }
        }
        info!("spot_rx closed: total batches = {}, total klines = {}", batch_count, total_klines);
    });

    info!("================fetch spot btc==============");

    let swap_kline_fetch = SimpleHistoryFetcher::kline(&SWAP_KLINE_HISTORY_COMMAND);
    let base_param = CommonRequestBuilder::new(symbol.to_string(), 1000, HistoryInterval::OneHour);

    let (swap_recipient, mut swap_rx) = mpsc::channel::<QueryCommand<BinanceKline>>(100);
    let _ = swap_kline_fetch
        .get_all_kline_data(base_param, Some(HistoryInterval::OneHour), Some(start_ms), None, swap_recipient, false)
        .await;
    let swap_collector = tokio::spawn(async move {
        let mut batch_count: usize = 0;
        let mut total_klines: usize = 0;
        while let Some(cmd) = swap_rx.recv().await {
            match cmd {
                QueryCommand::BatchInsert(payload) => {
                    let symbol = payload.symbol.clone();
                    let size = payload.data.len();
                    info!("received BatchInsert for {:?}, size={}", symbol, size);
                    if size > 0 {
                        // 打印该批次的首尾时间信息
                        print_kline_result(&payload.data, Some(HistoryInterval::FiveMinutes));
                    }
                    batch_count += 1;
                    total_klines += size;
                    // 如果需要给发送方回复，可以使用 payload.result_tx.send(...)
                    // 目前这里不回复，直接让 result_tx 被丢弃。
                }
                QueryCommand::GetCount(resp) => {
                    // 返回当前已统计的总数（示例中使用 usize -> Result<usize, YueError>）
                    let _ = resp.send(Ok(total_klines));
                }
            }
        }
        info!("spot_rx closed: total batches = {}, total klines = {}", batch_count, total_klines);
    });
    info!("================fetch swap btc==============");
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

    // 等待 collector 任务完成，确保打印最终结果
    let _ = spot_collector.await;
    let _ = swap_collector.await;
    // let swap_funding_rate_fetch = SimpleHistoryFetcher::new(&SWAP_FUNDING_RATE_COMMAND);
    // let base_param = CommonParam::new(symbol.to_string(), 1000, HistoryInterval::OneHour);
    // let btc_funding_rate: Result<(Vec<FundingRate>, u16), YueError> =
    //     swap_funding_rate_fetch.get_all_kline_data(base_param, None, Some(start_ms), None).await;
    // info!("================fetch btc funding rate ==============");
    // print_kline_result(&btc_funding_rate, Some(HistoryInterval::OneHour));
}
