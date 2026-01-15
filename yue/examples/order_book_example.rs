/// OrderBookService 示例
///
/// 这个示例展示如何：
/// 1. 创建并启动 OrderBookService
/// 2. 连接到币安 WebSocket 获取深度更新数据
/// 3. 订阅 OrderBook 快照并打印
/// 4. 自动处理订单簿的初始化和增量更新
use actix::{Actor, Context, Handler};
use li::tools::logs::setup_logger;
use log::{LevelFilter, info};
use serde_json::to_string;
use std::collections::HashMap;
use yue::binance::bn_json_websocket::{SPOT_STREAM_WEBSOCKET, StreamCommandRequest, WS_SUBSCRIBE_COMMAND};
use yue::binance::order_book::{OrderBookService, OrderBookSnapshotMsg, Subscribe};
use yue::binance::parsers::BinanceSpotStreamParser;
use yue::http_client::init_http_client;
use yue::websocket::client::{SendTextMessage, SubscribeToEvents, WebSocketClient, WebSocketEvent};
use yue::websocket::event_bus::WsMessageBus;

/// 打印订单簿快照的订阅者 Actor
#[derive(Debug)]
pub struct OrderBookPrinterActor {
    print_count: usize,
}

impl OrderBookPrinterActor {
    pub fn new() -> Self {
        Self { print_count: 0 }
    }
}

impl Actor for OrderBookPrinterActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("OrderBookPrinterActor 已启动: {:?}", self);
    }
}

impl Handler<OrderBookSnapshotMsg> for OrderBookPrinterActor {
    type Result = ();

    fn handle(&mut self, msg: OrderBookSnapshotMsg, _ctx: &mut Context<Self>) {
        self.print_count += 1;
        let order_book = &msg.0;

        info!("========== 订单簿快照 #{} ==========", self.print_count);
        info!("交易对: {}", order_book.symbol);
        info!("更新ID: {}", order_book.local_update_id);
        info!("更新时间: {}", order_book.last_update_time);

        // 打印最优买卖价
        if let Some((best_bid_price, best_bid_qty)) = order_book.best_bid() {
            info!("\n最优买价 (Best Bid): {} (数量: {})", best_bid_price, best_bid_qty);
        }
        if let Some((best_ask_price, best_ask_qty)) = order_book.best_ask() {
            info!("最优卖价 (Best Ask): {} (数量: {})", best_ask_price, best_ask_qty);
        }

        // 计算价差
        if let (Some((best_bid_price, _)), Some((best_ask_price, _))) = (order_book.best_bid(), order_book.best_ask()) {
            let spread = best_ask_price - best_bid_price;
            let spread_bps = if *best_ask_price > rust_decimal::Decimal::ZERO {
                (spread / best_ask_price) * rust_decimal::Decimal::new(10000, 0)
            } else {
                rust_decimal::Decimal::ZERO
            };
            info!("价差 (Spread): {} ({:.2} bps)", spread, spread_bps);
        }

        info!("\n订单簿总档位 - Bids: {}, Asks: {}", order_book.bids_count(), order_book.asks_count());
        info!("==========================================\n");
    }
}

#[actix::main]
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
    init_http_client(proxy);
    info!("✓ HTTP 客户端已初始化");

    // 步骤 1: 创建并启动 OrderBookService
    // market_depth 设置为 20，表示广播时只保留前 20 档
    let order_book_service = OrderBookService::new().with_market_depth(20).start();
    info!("✓ OrderBookService 已启动");

    // 步骤 2: 创建打印订阅者
    let printer = OrderBookPrinterActor::new().start();

    // 步骤 3: 订阅 OrderBook 快照
    order_book_service.do_send(Subscribe {
        recipient: printer.recipient(),
    });
    info!("✓ OrderBookPrinterActor 已订阅");

    // 步骤 4: 创建 WebSocket 客户端连接到币安
    // 如果需要代理，取消下面的注释并设置正确的代理地址
    let client_addr = WebSocketClient::new(SPOT_STREAM_WEBSOCKET)
        .with_reconnect_interval(std::time::Duration::from_secs(5))
        .with_proxy("http://127.0.0.1:7891") // 取消注释以启用代理
        .start();
    info!("✓ WebSocket 客户端已启动");

    // 步骤 5: 创建消息总线，将 WebSocket 消息解析并分发
    let bus = WsMessageBus::new(BinanceSpotStreamParser).start();
    info!("✓ WsMessageBus 已启动");

    // 步骤 6: 将 OrderBookService 注册为 bus 的订阅者
    // 这样 OrderBookService 就能收到解析后的深度更新消息
    bus.do_send(yue::websocket::event_bus::Subscribe {
        subscriber: order_book_service.recipient(),
    });
    info!("✓ OrderBookService 已订阅 WsMessageBus");

    // 步骤 7: 将 bus 注册为 WebSocket 客户端的事件接收者
    client_addr
        .send(SubscribeToEvents {
            recipient: bus.recipient::<WebSocketEvent>(),
        })
        .await??;
    info!("✓ WsMessageBus 已订阅 WebSocket 事件");

    // 等待连接建立
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // 步骤 8: 订阅 BTCUSDT 的深度更新流
    info!("\n📤 订阅 BTCUSDT 深度更新流 (depth@100ms)");
    let subscribe_request = StreamCommandRequest {
        method: WS_SUBSCRIBE_COMMAND.to_string(),
        params: vec!["btcusdt@depth@100ms".to_string()],
        id: 1,
    };
    client_addr
        .send(SendTextMessage {
            text: to_string(&subscribe_request)?,
        })
        .await??;
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
    client_addr
        .send(SendTextMessage {
            text: to_string(&subscribe_eth)?,
        })
        .await??;
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
