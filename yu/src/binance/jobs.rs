use crate::binance::binance_db_consts::ALL_BINANCE_TABLES;
use crate::binance::bn_backend_service::{get_spot_kline_table, get_spot_order_book_service, get_spot_trading_service, get_swap_kline_table};
use crate::binance::bn_dashboard::{BinanceDashboard, BinanceDashboardWatcher};
use crate::binance::bn_data_integrity::{KlineGapRepairStrategy, SpotCheckStrategy};
use crate::binance::history::{initial_kline, start_sync_funding_rate};
use crate::binance::websocket_service::KlineSubscribeService;
use crate::config::{get_config, AccountType, AppConfig, SecurityType};
use crate::cron_job;
use crate::data_integrity::check::ValidationStrategyTrait;
use crate::data_integrity::models::RepairRequest;
use crate::data_integrity::repair::RepairStrategyTrait;
use crate::duck_db::DBProvider;
use crate::errors::YuError;
use log::{error, info, warn};
use std::sync::Arc;
use tokio::sync::watch;
use yue::binance::bn_models::common::SymbolType;
use yue::models::HistoryInterval;

///
/// NEXT: 加入的功能
/// 1. 检测数据完整性的进程。
/// 2. 初始化并行执行。
///     - spot和swap的kline阻塞
///     - funding rate非阻塞
///
///
pub async fn start_bn_jobs() -> Result<(), YuError> {
    let config = get_config();
    let dash_board = Arc::new(BinanceDashboard::new(config.get_data_retention_hours()));
    let snapshot = dash_board.execute().await?;
    let (dash_board_watch, _) = watch::channel(snapshot);
    let dash_board_refresh = dash_board.clone();
    let dashboard_watch_sender = dash_board_watch.clone();
    let _ = cron_job!("0 01 * * * *", move |_uuid, _locked| {
        let dash_board_job = dash_board_refresh.clone();
        let dashboard_watch_refresher = dashboard_watch_sender.clone();
        Box::pin(async move {
            info!("start refresh binance exchange info");
            match dash_board_job.clone().execute().await {
                Ok(snapshot) => {
                    if let Err(e) = dashboard_watch_refresher.send(snapshot) {
                        error!("Failed to send updated snapshot to channel: {}", e);
                    } else {
                        info!("BinanceDashboard snapshot updated and sent to channel");
                    }
                }
                Err(_) => {
                    error!("Failed to refresh binance dash_board");
                }
            }
        })
    });

    if let Err(_e) = initial_tables(None) {
        warn!("币安表创建失败,{}", _e);
    }
    info!("数据库创建表完成");
    start_refresh_history_data(dash_board.clone(), config, dash_board_watch.clone()).await?;
    // start_monitor_account().await?;
    start_spot_websocket_stream_job().await?;
    start_data_integrity_jobs(config).await?;
    Ok(())
}

//开始数据监控的job
pub async fn start_data_integrity_jobs(config: &AppConfig) -> Result<(), YuError> {
    let data_integrity_config = config.get_data_integrity_config();
    let data_retention_time = config.get_data_retention_hours();
    let _ = cron_job!(data_integrity_config.periodic_check_interval_cron, move |_uuid, _locked| {
        info!("start periodic data integrity check for binance kline data");
        Box::pin(async move {
            let repair_spot_kline_strategy = KlineGapRepairStrategy::spot();
            let repair_swap_kline_strategy = KlineGapRepairStrategy::swap();
            let check_spot_kline_strategy = SpotCheckStrategy::spot_check_strategy(None, data_retention_time);
            let check_swap_kline_strategy = SpotCheckStrategy::swap_check_strategy(None, data_retention_time);
            info!("finish periodic data integrity check for binance spot kline data");
            match check_spot_kline_strategy.validate().await {
                Ok(Some(gaps)) => {
                    for g in &gaps.gaps {
                        println!("{:?}", g);
                    }

                    let repair_request = RepairRequest {
                        id: 0,
                        strategy: "spot".to_string(),
                        gaps: gaps.gaps,
                    };
                    if let Err(e) = repair_spot_kline_strategy.repair(repair_request).await {
                        error!("Failed to repair binance swap: {}", e);
                    }
                }
                Err(_) => {}
                _ => {}
            }
            info!("finish periodic data integrity check for binance swap kline data");
            match check_swap_kline_strategy.validate().await {
                Ok(Some(gaps)) => {
                    for g in &gaps.gaps {
                        println!("{:?}", g);
                    }

                    let repair_request = RepairRequest {
                        id: 0,
                        strategy: "spot".to_string(),
                        gaps: gaps.gaps,
                    };
                    if let Err(e) = repair_swap_kline_strategy.repair(repair_request).await {
                        error!("Failed to repair binance swap: {}", e);
                    }
                }
                Err(_) => {}
                _ => {}
            }
            info!("finish periodic data integrity check for binance kline data");
        })
    });

    Ok(())
}

///
/// 开始监控币安的账户。大致分成两大类。
///
/// 1. 统一账户：专门的去监听
/// 2. 一般账户：
///    spot： spot的webstream
///    swap： swap的webstream
///
/// 基本流程
/// 1. 通过account_type。把account_type分成两个两组
///
///
pub async fn start_monitor_account() -> Result<(), YuError> {
    let config = get_config();

    let (binance_normal_infos, binance_portfolio_infos) = config
        .binance
        .as_ref()
        .and_then(|ws| ws.accounts.as_ref())
        .map(|accounts| {
            let mut normal = Vec::new();
            let mut portfolio = Vec::new();

            for acc in accounts.iter().filter(|a| a.secret_type == SecurityType::Ed25519) {
                let info = acc.clone(); // Into\<SpotStreamAccountWebsocketInfo\>
                match acc.account_type {
                    AccountType::BinanceNormal => normal.push(info),
                    AccountType::BinancePortfolio => portfolio.push(info),
                }
            }

            (normal, portfolio)
        })
        .unwrap_or_default();

    // 示例：打印各组数量（可按需替换为后续逻辑）
    info!(
        "start to monitor binance account: normal={}, portfolio={},",
        binance_normal_infos.len(),
        binance_portfolio_infos.len(),
    );
    Ok(())
}

/// 启动后台的websocket任务，然后根据配置来配置需要的内容
/// 1. 启动websocket客户端。监听以下内容
///    - spot stream
/// 2. 启动WsMessageBus，订阅websocket客户端的事件，分发给不同的订阅者
/// 3. SpotStreamStorageActor，订阅启动WsMessageBus信息
/// 4，根据配置信息，启动一个专门管理spot的OrderBookService
async fn start_spot_websocket_stream_job() -> Result<(), YuError> {
    let config = get_config();
    // 检查是否启用了 WebSocket 功能
    let ws_config = match &config.binance {
        Some(ws) => ws,
        None => {
            info!("binance_websocket 配置未启用，跳过 WebSocket 任务");
            return Ok(());
        }
    };

    let spot_config = match &ws_config.spot_stream {
        Some(spot) => spot,
        None => {
            info!("binance_websocket.spot 配置未启用，跳过 Spot WebSocket 任务");
            return Ok(());
        }
    };

    let order_service = get_spot_order_book_service().await;
    // 根据配置订阅深度流
    if let Some(depth_config) = &spot_config.depth {
        if depth_config.enabled() && !depth_config.symbols.is_empty() {
            let update_speed = &depth_config.update_speed();
            for symbol in depth_config.symbols.iter() {
                if let Err(e) = order_service.subscribe_order_book(symbol, update_speed) {
                    error!("在订阅{},speed:{}的订单簿时出错{}", symbol, update_speed, e);
                    continue;
                }
                info!("订阅{}:{}的订单簿", symbol, update_speed);
            }
        } else {
            info!("binance_websocket.spot.depth 未启用或没有配置symbols，跳过深度流订阅");
        }
    } else {
        info!("binance_websocket.spot.depth 配置未启用，跳过深度流订阅");
    }

    if let Some(trade_config) = &spot_config.trade {
        if trade_config.enabled.unwrap() && !trade_config.symbols.is_empty() {
            let trading_service = get_spot_trading_service().await;
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
    }

    Ok(())
}

///
/// # 关于K线的业务思考。
/// ## 初始化和更新的区别
/// 1. 初始化使用restful，而更新使用websocket。因为初始化需要获取历史数据，而更新只需要获取最新数据。
/// 2. symbol的区别。只是更新在trading的数据。但是初始化，一些下架币也要更新(待定)
///
/// # 基本流程。
/// 1. 获取当前时间。
/// 2. 开始监听websocket，获取最新的kline数据，并且更新到数据库中。
/// 3。开始同步历史数据。完成后返回。同步历史数据
///
/// ## 同步历史数据。
/// 1. symbol获取所有的历史数据。
/// 2  然后通过config的data_retention_hours到现在来获取。
///
/// FUTURE
/// 1. 直接去aws下载文本数据，然后再考虑处理。
///
async fn start_refresh_history_data(
    dash_board: Arc<BinanceDashboard>,
    config: &AppConfig,
    dash_board_watch: BinanceDashboardWatcher,
) -> Result<(), YuError> {
    let interval = HistoryInterval::FiveMinutes;

    //开始websocket监听
    let spot_kline_table = get_spot_kline_table();
    let proxy = config.proxy_url.clone();

    if let Err(e) = KlineSubscribeService::startup_spot(spot_kline_table.clone(), dash_board_watch.clone(), proxy.clone(), interval.clone()).await {
        error!("error starting kline service: {}", e);
    }

    let swap_kline_table = get_swap_kline_table();
    if let Err(e) = KlineSubscribeService::startup_swap(swap_kline_table.clone(), dash_board_watch.clone(), proxy, interval.clone()).await {
        error!("error starting swap kline service: {}", e);
    }
    let spot_all = dash_board.spot_all_symbols();
    let swap_all = dash_board.swap_all_symbols();
    let spot_symbol: Vec<String> = spot_all
        .read()
        .unwrap()
        .iter()
        .filter(|s| s.quote_asset == "USDT")
        .map(|s| s.symbol.clone())
        .collect();
    let swap_symbol: Vec<String> = swap_all
        .read()
        .unwrap()
        .iter()
        .filter(|s| s.quote_asset == "USDT")
        .map(|s| s.symbol.clone())
        .collect();
    initial_kline(SymbolType::Spot, spot_symbol, config, interval, spot_kline_table).await?;
    initial_kline(SymbolType::Swap, swap_symbol.clone(), config, HistoryInterval::OneHour, swap_kline_table).await?;

    start_sync_funding_rate(swap_symbol, config, HistoryInterval::OneHour, dash_board_watch).await?;
    Ok(())
}

pub fn initial_tables(provider: Option<DBProvider>) -> Result<(), YuError> {
    let db_provider = provider.unwrap_or_else(|| DBProvider::default());
    let conn = db_provider.acquire()?;
    for table in ALL_BINANCE_TABLES.iter() {
        let create_sql = table.create_table_statement();
        let table_initial_stmt = create_sql.split(';');
        for stmt in table_initial_stmt {
            let sql = stmt.trim();
            if !sql.is_empty() {
                conn.execute(sql, [])?;
            }
        }
    }
    info!("initial binance tables done");
    Ok(())
}
