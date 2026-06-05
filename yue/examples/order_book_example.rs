/// OrderBookService 示例
///
/// 这个示例展示如何：
/// 1. 创建并启动 OrderBookService
/// 2. 连接到币安 WebSocket 获取深度更新数据
/// 3. 订阅 OrderBook 快照并打印
/// 4. 自动处理订单簿的初始化和增量更新
use li::tools::logs::setup_logger;
use li::tools::time::unix_2_readable;
use log::{LevelFilter, error, info};
use std::collections::HashMap;
use yue::binance::bn_json_websocket::{StreamCommandRequest, WS_SUBSCRIBE_COMMAND};
use yue::binance::order_book::{OrderBookService, Subscribe};
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
    if let Err(e) = order_book_service.subscribe_order_book("ethusdt", "100ms") {
        error!("订阅失败{}", e);
    }
    info!("✓ OrderBookService 已启动");
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    info!("✓ 开始查询order book");
    if let Ok(eth_order_book) = order_book_service.query_order_book("ethusdt", 20).await {
        info!("order eth book received: {:?}", unix_2_readable(&eth_order_book.last_update_time));
        info!("order asks num:{},bids:{}", eth_order_book.asks().len(), eth_order_book.bids().len());
    }

    Ok(())
}
