/// OrderBookService 示例
///
/// 这个示例展示如何：
/// 1. 创建并启动 OrderBookService
/// 2. 连接到币安 WebSocket 获取深度更新数据
/// 3. 订阅 OrderBook 快照并打印
/// 4. 自动处理订单簿的初始化和增量更新
use actix::{Actor, Context, Handler};
use li::subscribe_event_addr;
use li::tools::logs::setup_logger;
use li::websocket::client::{CommandMessage, WebSocketClient};
use li::websocket::connection::WebSocketEvent::Error;
use log::{LevelFilter, error, info};
use serde_json::to_string;
use std::collections::HashMap;
use yue::binance::bn_json_websocket::{SPOT_STREAM_WEBSOCKET, StreamCommandRequest, WS_SUBSCRIBE_COMMAND};
use yue::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamResponse;
use yue::binance::order_book::{OrderBookService, OrderBookSnapshotMsg, Subscribe};
use yue::http_client::init_http_client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    let mut special_log = HashMap::new();
    special_log.insert("yue".to_string(), LevelFilter::Info);
    special_log.insert("li".to_string(), LevelFilter::Info);
    special_log.insert("order_book_example".to_string(), LevelFilter::Info);
    setup_logger(Some(LevelFilter::Warn), special_log)?;

    info!("========== OrderBookService 示例 ==========");
    info!("本示例展示如何使用 OrderBookService 维护实时订单簿");

    // 初始化 HTTP 客户端（用于获取初始深度数据）
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy.clone());
    info!("✓ HTTP 客户端已初始化");

    // 步骤 1: 创建并启动 OrderBookService
    // market_depth 设置为 20，表示广播时只保留前 20 档
    let order_book_service = OrderBookService::spot(Some(proxy.unwrap().to_string())).await;
    info!("✓ OrderBookService 已启动");
    if let Err(e) = order_book_service.subscribe_order_book("ethusdt", "100ms") {
        error!("{}", e);
    }

    info!("✓ 订阅请求已发送");

    info!("\n等待订单簿数据...");
    info!("OrderBookService 将自动:");
    info!("  1. 检测到新的 symbol (BTCUSDT)");
    info!("  2. 通过 RESTful API 获取初始深度快照");
    info!("  3. 应用 WebSocket 增量更新");
    info!("  4. 广播订单簿快照给所有订阅者\n");

    // 运行 30 秒，观察订单簿更新
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

    // 可选：订阅更多交易对
    info!("\n📤 订阅 ETHUSDT 深度更新流");
    let subscribe_eth = StreamCommandRequest {
        method: WS_SUBSCRIBE_COMMAND.to_string(),
        params: vec!["ethusdt@depth@100ms".to_string()],
        id: 2,
    };
    info!("✓ ETHUSDT 订阅请求已发送");

    // 再运行 30 秒
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

    info!("\n========== 示例结束 ==========");
    info!("提示:");
    info!("  - OrderBookService 自动管理所有 symbol 的订单簿");
    info!("  - 支持多个订阅者同时接收订单簿快照");
    info!("  - 自动处理订单簿过期和重新初始化");
    info!("  - 可通过 with_market_depth() 设置广播的档位数");

    Ok(())
}
