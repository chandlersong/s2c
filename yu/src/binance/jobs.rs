use crate::binance::binance_db_consts::BinanceTables::{SpotKline, SwapFundingRate, SwapKline};
use crate::binance::binance_db_consts::ALL_BINANCE_TABLES;
use crate::binance::bn_dashboard::{init_market_depth_dashboard, BinanceDashboard, MarketDepthDashBoard};
use crate::binance::history_task::{DuckDBHistoryDataWriter, HistoryDataTask};
use crate::binance::models::po::KlinePo;
use crate::config::{get_config, AccountConfig, SecurityType};
use crate::duck_db::DBProvider;
use crate::errors::YuError;
use crate::exchange::CloneHistoryFetcherFactory;
use crate::websocket::subscribers::storage_subscriber::get_spot_stream_writer;
use crate::websocket::subscribers::AccountSyncActor;
use actix::Actor;
use li::actix_jobs::{AsyncRepeatTask, CronActor};
use log::{info, warn};
use rust_decimal::prelude::ToPrimitive;
use serde_json::to_string;
use std::sync::Arc;
use yue::binance::bn_json_websocket::{StreamCommandRequest, SPOT_STREAM_WEBSOCKET, SPOT_WEBSOCKET, WS_SUBSCRIBE_COMMAND};
use yue::binance::bn_models::common::SymbolType;
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_restful_commands::{
    SPOT_KLINE_HISTORY_COMMAND, SWAP_FIVE_MIN_KLINE_HISTORY_COMMAND, SWAP_FUNDING_RATE_COMMAND, SWAP_KLINE_HISTORY_COMMAND,
};
use yue::binance::history_data::{CommonParam, SimpleHistoryFetcher};
use yue::binance::order_book::{OrderBookService, Subscribe as OrderBookSubscribe};
use yue::binance::websocket_handler::{BinanceSpotStreamHandler, SpotAccountStreamHandler};
use yue::models::HistoryInterval;
use yue::tools::SnowyFlakeWrapper;
use yue::websocket::client_deprecated::{SendTextMessage, SubscribeToEvents, WebSocketClient, WebSocketEvent};
use yue::websocket::event_bus::{Subscribe, WsMessageBus};

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
    let dash_board = BinanceDashboard::new(config.get_data_retention_hours());
    dash_board.initial_data().await?;
    if let Err(_e) = initial_tables(None) {
        warn!("币安表创建失败,{}", _e);
    }
    info!("数据库创建表完成");
    start_refresh_history_data(dash_board.clone()).await?;
    start_monitor_account().await?;
    start_spot_websocket_stream_job().await?;
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
///
///
///
pub async fn start_monitor_account() -> Result<(), YuError> {
    let config = get_config();
    let mut client_builder = WebSocketClient::new(SPOT_WEBSOCKET);

    if let Some(proxy) = &config.proxy_url {
        client_builder = client_builder.with_proxy(proxy);
        info!("✓ WebSocket 使用代理: {}", proxy);
    }
    let client_addr = client_builder.with_reconnect_interval(std::time::Duration::from_secs(5)).start();
    info!("✓ WebSocket 客户端已启动: {}", SPOT_WEBSOCKET);

    let acc_infos = get_config()
        .binance
        .as_ref()
        .and_then(|ws| ws.accounts.as_ref())
        .map(|accounts| {
            accounts
                .iter()
                .filter_map(|acc: &AccountConfig| {
                    if acc.secret_type == SecurityType::Ed25519 {
                        info!("账户{}开始监听", acc.account_name);
                        Some(acc.clone().into())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let handler = SpotAccountStreamHandler::new(acc_infos);
    let bus = WsMessageBus::new(handler).start();
    let account_sync_add = AccountSyncActor::new(None).start();

    bus.do_send(Subscribe {
        subscriber: account_sync_add.recipient(),
    });

    info!("✓ WsMessageBus started");

    client_addr
        .send(SubscribeToEvents {
            recipient: bus.recipient::<WebSocketEvent>(),
        })
        .await
        .map_err(|e| YuError::CustomError(format!("发送订阅事件失败: {}", e)))??;
    info!("✓ WsMessageBus 订阅 WebSocketClient 事件");
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

    let trade_config = match &spot_config.trade {
        Some(trade) => trade,
        None => {
            info!("binance_websocket.spot.trade 配置未启用，跳过 Spot Trade WebSocket 任务");
            return Ok(());
        }
    };

    if !trade_config.enabled.unwrap_or(false) {
        info!("binance_websocket.spot.trade.enabled = false，跳过 Spot Trade WebSocket 任务");
        return Ok(());
    }

    // 步骤1: 启动 WebSocket 客户端
    let mut client_builder = WebSocketClient::new(SPOT_STREAM_WEBSOCKET);

    if let Some(proxy) = &config.proxy_url {
        client_builder = client_builder.with_proxy(proxy);
        info!("✓ WebSocket 使用代理: {}", proxy);
    }

    let client_addr = client_builder.with_reconnect_interval(std::time::Duration::from_secs(5)).start();
    info!("✓ WebSocket 客户端已启动: {}", SPOT_STREAM_WEBSOCKET);

    // 步骤2: 启动 WsMessageBus
    let bus = WsMessageBus::new(BinanceSpotStreamHandler).start();
    info!("✓ WsMessageBus 已启动");

    // 步骤3: 启动 SpotStreamStorageActor
    let storage_actor = get_spot_stream_writer();
    info!("✓ SpotStreamStorageActor 已启动");

    // 订阅 WsMessageBus 到存储 Actor
    bus.do_send(Subscribe {
        subscriber: storage_actor.recipient(),
    });
    info!("✓ SpotStreamStorageActor 已订阅 WsMessageBus");

    // 步骤2.5: 只有开启 depth 时，才初始化 OrderBookService 和 MarketDepthDashBoard
    if let Some(depth_config) = &spot_config.depth {
        if depth_config.enabled() && !depth_config.symbols.is_empty() {
            let depth = depth_config.levels.unwrap_or(20).to_u16().unwrap_or_else(|| 20);
            let order_book_service = OrderBookService::new().with_market_depth(depth).start();
            info!("✓ OrderBookService 已启动 (market_depth=20)");

            let market_depth_dashboard = MarketDepthDashBoard::new().start();
            info!("✓ MarketDepthDashBoard 已启动");

            // 初始化全局MarketDepthDashBoard单例
            if let Err(_) = init_market_depth_dashboard(market_depth_dashboard.clone()) {
                info!("⚠ MarketDepthDashBoard已初始化过，跳过重复初始化");
            }
            info!("✓ 全局MarketDepthDashBoard单例已初始化");

            // MarketDepthDashBoard 订阅 OrderBookService 的订单簿快照
            order_book_service.do_send(OrderBookSubscribe {
                recipient: market_depth_dashboard.recipient(),
            });
            info!("✓ MarketDepthDashBoard 已订阅 OrderBookService");

            // OrderBookService 订阅 WsMessageBus 的深度更新
            bus.do_send(Subscribe {
                subscriber: order_book_service.recipient(),
            });
            info!("✓ OrderBookService 已订阅 WsMessageBus");
        } else {
            info!("binance_websocket.spot.depth 未启用或没有配置symbols，跳过OrderBookService初始化");
        }
    } else {
        info!("binance_websocket.spot.depth 配置未启用，跳过OrderBookService初始化");
    }

    // 订阅 WebSocketClient 事件到 WsMessageBus
    client_addr
        .send(SubscribeToEvents {
            recipient: bus.recipient::<WebSocketEvent>(),
        })
        .await
        .map_err(|e| YuError::CustomError(format!("发送订阅事件失败: {}", e)))??;
    info!("✓ WsMessageBus 已订阅 WebSocketClient 事件");

    // 等待连接建立
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // 构建订阅参数
    let mut params = Vec::new();
    for symbol in &trade_config.symbols {
        // 转换为小写并添加 @trade 后缀
        let stream = format!("{}@trade", symbol.to_lowercase());
        params.push(stream);
    }
    let snow_flake = SnowyFlakeWrapper::new();
    if !params.is_empty() {
        info!("📤 订阅交易流: {:?}", params);
        let subscribe_request = StreamCommandRequest {
            method: WS_SUBSCRIBE_COMMAND.to_string(),
            params,
            id: snow_flake.next_id_u64(),
        };

        client_addr
            .send(SendTextMessage::new(
                to_string(&subscribe_request).map_err(|e| YuError::CustomError(format!("序列化订阅请求失败: {}", e)))?,
            ))
            .await
            .map_err(|e| YuError::CustomError(format!("发送订阅消息失败: {}", e)))??;
        info!("✓ 交易流订阅请求已发送");
    }

    // 根据配置订阅深度流
    if let Some(depth_config) = &spot_config.depth {
        if depth_config.enabled() && !depth_config.symbols.is_empty() {
            let mut depth_params = Vec::new();
            let update_speed = depth_config.update_speed();

            for symbol in &depth_config.symbols {
                // 转换为小写并添加深度流后缀
                // 格式: btcusdt@depth20@100ms 或 btcusdt@depth@100ms
                depth_params.push(format!("{}@depth@{}", symbol.to_lowercase(), update_speed));
            }

            if !depth_params.is_empty() {
                info!(
                    "📤 订阅深度流: {:?} (update_speed={}, levels={})",
                    depth_params,
                    depth_config.update_speed(),
                    depth_config.levels()
                );
                let depth_subscribe_request = StreamCommandRequest {
                    method: WS_SUBSCRIBE_COMMAND.to_string(),
                    params: depth_params,
                    id: snow_flake.next_id_u64(),
                };

                client_addr
                    .send(SendTextMessage::new(
                        to_string(&depth_subscribe_request).map_err(|e| YuError::CustomError(format!("序列化深度订阅请求失败: {}", e)))?,
                    ))
                    .await
                    .map_err(|e| YuError::CustomError(format!("发送深度订阅消息失败: {}", e)))??;
                info!("✓ 深度流订阅请求已发送");
            }
        } else {
            info!("binance_websocket.spot.depth 未启用或没有配置symbols，跳过深度流订阅");
        }
    } else {
        info!("binance_websocket.spot.depth 配置未启用，跳过深度流订阅");
    }

    Ok(())
}

///
///
async fn start_refresh_history_data(origin_dash_board: BinanceDashboard) -> Result<(), YuError> {
    let update_dashboard_task = origin_dash_board.clone();
    let dash_board = Arc::new(origin_dash_board);

    let build_kline_task = |request_info, dash_board, data_writer, task_name: &str, symbol_type, interval| {
        let base_fetcher = SimpleHistoryFetcher::new(request_info);
        let fetcher_factory: CloneHistoryFetcherFactory<SimpleHistoryFetcher, CommonParam, BinanceKline> =
            CloneHistoryFetcherFactory::new(base_fetcher);
        HistoryDataTask::<_, _, KlinePo, BinanceKline, BinanceDashboard>::new(
            fetcher_factory,
            dash_board,
            data_writer,
            task_name.to_string(),
            symbol_type,
            Some(interval),
        )
    };

    let spot_data_writer = Arc::new(DuckDBHistoryDataWriter::new(DBProvider::default(), SpotKline));
    let spot_kline_task = build_kline_task(
        &SPOT_KLINE_HISTORY_COMMAND,
        dash_board.clone(),
        spot_data_writer,
        "refresh spot kline data",
        SymbolType::Spot,
        HistoryInterval::FiveMinutes,
    );
    spot_kline_task.initial_data().await?;

    //PLAN： 更新交易所时间表达式进入Config
    let _ = CronActor::new("30 59 */6 * * * *", update_dashboard_task).start();
    //FUTURE: 支持不同的interval
    let _ = CronActor::new("01 */5 * * * * *", spot_kline_task).start();

    let swap_data_writer = Arc::new(DuckDBHistoryDataWriter::new(DBProvider::default(), SwapKline));
    let swap_initial_kline_task = build_kline_task(
        &SWAP_KLINE_HISTORY_COMMAND,
        dash_board.clone(),
        swap_data_writer.clone(),
        "initial swap kline data",
        SymbolType::Swap,
        HistoryInterval::FiveMinutes,
    );
    swap_initial_kline_task.initial_data().await?;

    let swap_update_kline_task = build_kline_task(
        &SWAP_FIVE_MIN_KLINE_HISTORY_COMMAND,
        dash_board.clone(),
        swap_data_writer,
        "refresh swap kline data",
        SymbolType::Swap,
        HistoryInterval::FiveMinutes,
    );
    let _ = CronActor::new("01 */5 * * * * *", swap_update_kline_task).start();

    let funding_rate_writer = Arc::new(DuckDBHistoryDataWriter::new(DBProvider::default(), SwapFundingRate));
    let funding_rate_task = build_kline_task(
        &SWAP_FUNDING_RATE_COMMAND,
        dash_board.clone(),
        funding_rate_writer,
        "refresh swap funding rate",
        SymbolType::Swap,
        HistoryInterval::OneHour,
    );
    funding_rate_task.initial_data().await?;
    let _ = CronActor::new("01 01 * * * * *", funding_rate_task).start();
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
