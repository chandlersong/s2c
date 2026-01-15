use crate::actix_jobs::{AsyncRepeatTask, CronActor};
use crate::binance::binance_consts::BinanceTables::{SpotKline, SwapFundingRate, SwapKline};
use crate::binance::binance_consts::ALL_BINANCE_TABLES;
use crate::binance::bn_dashboard::{init_market_depth_dashboard, BinanceDashboard, MarketDepthDashBoard};
use crate::binance::history_task::{DuckDBHistoryDataWriter, FundingRatePo, InitialHistoryTask, KlinePo};
use crate::config::get_config;
use crate::duck_db::DBProvider;
use crate::errors::YuError;
use crate::exchange::CloneHistoryFetcherFactory;
use crate::utils::get_snowflake_generator;
use crate::websocket::binance_spot::create_spot_stream_tables;
use crate::websocket::subscribers::SpotStreamStorageActor;
use actix::Actor;
use duckdb::Connection;
use log::info;
use rust_decimal::prelude::ToPrimitive;
use serde_json::to_string;
use std::sync::Arc;
use yue::binance::bn_json_websocket::{StreamCommandRequest, SPOT_STREAM_WEBSOCKET, WS_SUBSCRIBE_COMMAND};
use yue::binance::bn_models::common::SymbolType;
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_models::swap_restful::FundingRate;
use yue::binance::bn_restful_commands::{SPOT_KLINE_HISTORY_COMMAND, SWAP_FUNDING_RATE_COMMAND, SWAP_KLINE_HISTORY_COMMAND};
use yue::binance::history_data::{CommonParam, SimpleHistoryFetcher};
use yue::binance::order_book::{OrderBookService, Subscribe as OrderBookSubscribe};
use yue::binance::parsers::BinanceSpotStreamParser;
use yue::websocket::client::{SendTextMessage, SubscribeToEvents, WebSocketClient, WebSocketEvent};
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
    let dash_board = BinanceDashboard::new();
    dash_board.execute().await?;

    start_refresh_history_data(dash_board.clone()).await?;
    start_websocket_job().await?;
    Ok(())
}
/// 启动后台的websocket任务，然后根据配置来配置需要的内容
/// 1. 启动websocket客户端。监听以下内容
///    - spot stream
/// 2. 启动WsMessageBus，订阅websocket客户端的事件，分发给不同的订阅者
/// 3. SpotStreamStorageActor，订阅启动WsMessageBus信息
/// 4，根据配置信息，启动一个专门管理spot的OrderBookService
async fn start_websocket_job() -> Result<(), YuError> {
    let config = get_config();

    // 检查是否启用了 WebSocket 功能
    let ws_config = match &config.binance_websocket {
        Some(ws) => ws,
        None => {
            info!("binance_websocket 配置未启用，跳过 WebSocket 任务");
            return Ok(());
        }
    };

    let spot_config = match &ws_config.spot {
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

    // 初始化数据库表
    create_spot_stream_tables()?;
    info!("✓ WebSocket 数据库表初始化完成");

    // 步骤1: 启动 WebSocket 客户端
    let mut client_builder = WebSocketClient::new(SPOT_STREAM_WEBSOCKET);

    if let Some(proxy) = &config.proxy_url {
        client_builder = client_builder.with_proxy(proxy);
        info!("✓ WebSocket 使用代理: {}", proxy);
    }

    let client_addr = client_builder.with_reconnect_interval(std::time::Duration::from_secs(5)).start();
    info!("✓ WebSocket 客户端已启动: {}", SPOT_STREAM_WEBSOCKET);

    // 步骤2: 启动 WsMessageBus
    let bus = WsMessageBus::new(BinanceSpotStreamParser).start();
    info!("✓ WsMessageBus 已启动");

    // 步骤3: 启动 SpotStreamStorageActor
    let storage_actor = SpotStreamStorageActor::new(spot_config.clone(), DBProvider::default()).start();
    info!("✓ SpotStreamStorageActor 已启动");

    // 订阅 WsMessageBus 到存储 Actor
    bus.do_send(Subscribe {
        subscriber: storage_actor.recipient(),
    });
    info!("✓ SpotStreamStorageActor 已订阅 WsMessageBus");

    // 步骤2.5: 只有开启 depth 时，才初始化 OrderBookService 和 MarketDepthDashBoard
    if let Some(depth_config) = &spot_config.depth {
        if depth_config.enabled() && !depth_config.symbols.is_empty() {
            let depth = match depth_config.levels.unwrap_or(20).to_u16() {
                None => 20,
                Some(v) => v,
            };
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

    if !params.is_empty() {
        info!("📤 订阅交易流: {:?}", params);
        let subscribe_request = StreamCommandRequest {
            method: WS_SUBSCRIBE_COMMAND.to_string(),
            params,
            id: get_snowflake_generator().lock().unwrap().real_time_generate().to_u64().unwrap(),
        };

        client_addr
            .send(SendTextMessage {
                text: to_string(&subscribe_request).map_err(|e| YuError::CustomError(format!("序列化订阅请求失败: {}", e)))?,
            })
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
                    id: get_snowflake_generator().lock().unwrap().real_time_generate().to_u64().unwrap(),
                };

                client_addr
                    .send(SendTextMessage {
                        text: to_string(&depth_subscribe_request).map_err(|e| YuError::CustomError(format!("序列化深度订阅请求失败: {}", e)))?,
                    })
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

async fn start_refresh_history_data(origin_dash_board: BinanceDashboard) -> Result<(), YuError> {
    let update_dashboard_task = origin_dash_board.clone();
    let dash_board = Arc::new(origin_dash_board);
    initial_history_table()?;
    let base_spot_kline_fetcher = SimpleHistoryFetcher::new(&SPOT_KLINE_HISTORY_COMMAND);
    let spot_kline_fetcher: CloneHistoryFetcherFactory<SimpleHistoryFetcher, CommonParam, BinanceKline> =
        CloneHistoryFetcherFactory::new(base_spot_kline_fetcher);

    let spot_data_writer = Arc::new(DuckDBHistoryDataWriter::new(DBProvider::default(), SpotKline, SymbolType::Spot));

    let spot_kline_task = InitialHistoryTask::<_, _, KlinePo, BinanceKline, BinanceDashboard>::new(
        spot_kline_fetcher,
        dash_board.clone(),
        spot_data_writer,
        "refresh spot kline data".to_string(),
    );
    spot_kline_task.execute().await?;

    let base_swap_kline_fetcher = SimpleHistoryFetcher::new(&SWAP_KLINE_HISTORY_COMMAND);
    let swap_kline_fetcher: CloneHistoryFetcherFactory<SimpleHistoryFetcher, CommonParam, BinanceKline> =
        CloneHistoryFetcherFactory::new(base_swap_kline_fetcher);
    let swap_kline_writer = Arc::new(DuckDBHistoryDataWriter::new(DBProvider::default(), SwapKline, SymbolType::Swap));
    let swap_kline_task = InitialHistoryTask::<_, _, KlinePo, BinanceKline, BinanceDashboard>::new(
        swap_kline_fetcher,
        dash_board.clone(),
        swap_kline_writer,
        "refresh swap kline data".to_string(),
    );
    swap_kline_task.execute().await?;

    //NEXT: 写一个资金费率的专用的param
    let base_swap_funding_rate_fetcher = SimpleHistoryFetcher::new(&SWAP_FUNDING_RATE_COMMAND);
    let swap_funding_rate_fetcher: CloneHistoryFetcherFactory<SimpleHistoryFetcher, CommonParam, FundingRate> =
        CloneHistoryFetcherFactory::new(base_swap_funding_rate_fetcher);
    let swap_funding_rate_writer = Arc::new(DuckDBHistoryDataWriter::new(DBProvider::default(), SwapFundingRate, SymbolType::Swap));
    let swap_funding_rate_task = InitialHistoryTask::<_, _, FundingRatePo, FundingRate, BinanceDashboard>::new(
        swap_funding_rate_fetcher,
        dash_board.clone(),
        swap_funding_rate_writer,
        "refresh swap funding rate".to_string(),
    );
    swap_funding_rate_task.execute().await?;
    //PLAN： 更新交易所时间表达式进入Config
    let _ = CronActor::new("30 59 */6 * * * *", update_dashboard_task).start();
    let _ = CronActor::new("10 0 * * * * *", spot_kline_task).start();
    let _ = CronActor::new("10 0 * * * * *", swap_funding_rate_task).start();
    let _ = CronActor::new("10 1 * * * * *", swap_kline_task).start();
    Ok(())
}

fn table_exists(conn: &Connection, table_name: &str) -> Result<bool, YuError> {
    let check_sql = format!("SELECT name FROM sqlite_master WHERE type='table' AND name='{}'", table_name);
    let mut stmt = conn.prepare(&check_sql)?;
    let mut rows = stmt.query([])?;
    Ok(rows.next()?.is_some())
}

fn initial_history_table() -> Result<(), YuError> {
    let conn = DBProvider::default().acquire()?;
    for table in ALL_BINANCE_TABLES.iter() {
        let table_name = table.table_name();
        if !table_exists(&conn, &table_name)? {
            // 表不存在，执行建表
            let create_sql = table.create_table_statement();
            let table_initial_stmt = create_sql.split(';');
            for stmt in table_initial_stmt {
                let sql = stmt.trim();
                if !sql.is_empty() {
                    conn.execute(sql, [])?;
                }
            }
        }
    }
    info!("initial binance tables done");
    Ok(())
}
