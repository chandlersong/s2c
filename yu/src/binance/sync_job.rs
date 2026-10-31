use crate::binance::bn_backend_service::{get_spot_kline_table, get_swap_kline_table};
use crate::binance::bn_dashboard::BinanceDashboard;
use crate::binance::db_consts::initial_tables;
use crate::binance::sync_server::{
    BinanceSpotSyncHistoryService, BinanceSpotTradeSyncHistoryService, BinanceSwapFundingRateSyncHistoryService, BinanceSwapSyncHistoryService,
    BinanceSyncInstrumentService,
};
use crate::binance::trading_service::TradingService;
use crate::binance::websocket_service::{KlineSubscribeService, SpotTradeSaver};
use crate::config::AppConfig;
use crate::cron_job;
use crate::errors::YuError;
use crate::sync::models::grpc_sync::{HistoryEntry, HistoryType};
use crate::sync::server::sync_server::{SyncHistoryService, SyncHistoryServiceTrait, SyncInstrumentService, SyncInstrumentServiceTrait};
use li::tools::time::{unix_2_readable, unix_time_now_u64_utc};
use log::{debug, error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

///
/// FUTURE:
/// 1. 通过aws的网盘下载历史数据，那个应该是最全的。
///

pub async fn start_binance_sync_server_job(app_config: &AppConfig, history_tx: mpsc::Sender<HistoryEntry>) -> Result<SyncInstrumentService, YuError> {
    let binance_config = app_config
        .binance
        .as_ref()
        .filter(|config| config.enable())
        .ok_or_else(|| YuError::ConfigError("Binance sync server is not configured or disabled".to_string()))?;

    // drop_tables(None)?;
    initial_tables(None)?;
    let interval = binance_config.get_kline_interval()?;
    info!(
        "Binance sync server job started with data retention hours: {},interval: {}",
        app_config.get_data_retention_hours(),
        interval.as_ref()
    );

    let dash_board = Arc::new(BinanceDashboard::new(app_config.get_data_retention_hours(), None));

    let history_service = BinanceSpotSyncHistoryService::for_production(
        app_config.get_data_retention_ms(),
        history_tx.clone(),
        dash_board.clone(),
        interval.clone(),
    );
    let swap_history_service = BinanceSwapSyncHistoryService::for_production(
        app_config.get_data_retention_ms(),
        history_tx.clone(),
        dash_board.clone(),
        interval.clone(),
    );
    let funding_rate_history_service =
        BinanceSwapFundingRateSyncHistoryService::for_production(dash_board.clone(), app_config.get_data_retention_ms(), history_tx.clone());
    let trade_history_service = BinanceSpotTradeSyncHistoryService::for_production();
    let mut support_history_service: HashMap<HistoryType, SyncHistoryService> = HashMap::new();
    support_history_service.insert(HistoryType::BinanceSpot, history_service.clone());
    support_history_service.insert(HistoryType::BinanceSwap, swap_history_service.clone());
    support_history_service.insert(HistoryType::BinanceSwapFundingRate, funding_rate_history_service.clone());
    support_history_service.insert(HistoryType::BinanceTrade, trade_history_service.clone());
    let instrument_service = BinanceSyncInstrumentService::for_production(dash_board.clone(), support_history_service);
    info!("start initial Binance Spot and Swap instruments");
    if let Err(error) = instrument_service.sync_instrument().await {
        error!("failed to sync Binance Spot and Swap instruments: {}", error);
    }
    info!("finished initial Binance Spot and Swap instruments data");
    trade_history_service.initial_history_data(None).await?;

    let start_initial_ms = unix_time_now_u64_utc();
    info!(
        "started initial Binance Spot kline history data,end time: {}",
        unix_2_readable(&start_initial_ms)
    );
    //
    // 先监听，在初始化，是为了防止丢失数据。
    //
    KlineSubscribeService::startup_spot_sync(
        get_spot_kline_table(),
        dash_board.clone(),
        app_config.proxy_url.clone(),
        interval.clone(),
        history_tx.clone(),
    )
    .await?;
    KlineSubscribeService::startup_swap_sync(
        get_swap_kline_table(),
        dash_board.clone(),
        app_config.proxy_url.clone(),
        interval,
        history_tx.clone(),
    )
    .await?;
    if let Some(spot_config) = &binance_config.spot_stream {
        if let Some(trade_config) = &spot_config.trade {
            if !trade_config.symbols.is_empty() {
                let trade_saver = SpotTradeSaver::for_production(dash_board.clone(), history_tx.clone()).await;

                let trading_service = TradingService::spot(app_config.proxy_url.clone(), trade_saver).await;
                for symbol in trade_config.symbols.iter() {
                    if let Err(e) = trading_service.subscribe_trade(symbol.clone()).await {
                        error!("在订阅{}的交易流时出错{}", symbol, e);
                        continue;
                    }
                    info!("订阅{}的交易流", symbol);
                }
            } else {
                info!("binance_websocket.spot.trade 未启用或没有配置symbols，跳过交易流订阅");
            }
        };
    };

    let funding_rate_history_sync = funding_rate_history_service.clone();
    let _ = cron_job!("30 0 * * * *", move |_uuid, _locked| {
        let each_fuding_rate_sync = funding_rate_history_sync.clone();
        Box::pin(async move {
            if let Err(error) = each_fuding_rate_sync.sync_all().await {
                error!("failed to check Binance Spotkline history: {}", error);
            }
        })
    });

    info!("started initial Binance Spot kline history data");
    history_service.initial_history_data(Some(start_initial_ms)).await?;
    info!("started initial Binance Swap kline history data");
    swap_history_service.initial_history_data(Some(start_initial_ms)).await?;
    info!("start initial Binance Swap funding rate history data");
    funding_rate_history_service.initial_history_data(Some(start_initial_ms)).await?;
    info!("started checking Binance Spot kline history data");
    history_service.check_history_data().await?;
    info!("started checking Binance Swap kline history data");
    swap_history_service.check_history_data().await?;
    trade_history_service.check_history_data().await?;

    let check_history_job = history_service.clone();
    let check_swap_history_job = swap_history_service.clone();
    let _ = cron_job!("0 23 */6 * * *", move |_uuid, _locked| {
        let each_spot_sync = check_history_job.clone();
        let each_swap_sync = check_swap_history_job.clone();
        Box::pin(async move {
            debug!("start checking Binance Spot and Swap kline history");
            if let Err(error) = each_spot_sync.check_history_data().await {
                error!("failed to check Binance Spotkline history: {}", error);
            }
            if let Err(error) = each_swap_sync.check_history_data().await {
                error!("failed to check Binance Swap kline history: {}", error);
            }
            debug!("finished checking Binance Spot and Swap kline history");
        })
    });
    let sync_instrument_job = instrument_service.clone();
    let _ = cron_job!("37 17 */6 * * *", move |_uuid, _locked| {
        let each_sync = sync_instrument_job.clone();
        Box::pin(async move {
            debug!("start syncing Binance Spot and Swap instruments");
            if let Err(error) = each_sync.sync_instrument().await {
                error!("failed to sync Binance Spot and Swap instruments: {}", error);
            } else {
                debug!("finished syncing Binance Spot and Swap instruments");
            }
        })
    });
    let funding_rate_history_check = funding_rate_history_service.clone();
    let _ = cron_job!("37 12 */6 * * *", move |_uuid, _locked| {
        let each_fuding_rate_sync = funding_rate_history_check.clone();
        Box::pin(async move {
            debug!("start check Binance Swap funding ");
            if let Err(error) = each_fuding_rate_sync.check_history_data().await {
                error!("failed to check Binance Swap funding rate history: {}", error);
            }
            debug!("finished check Binance Swap funding ");
        })
    });
    Ok(instrument_service)
}
