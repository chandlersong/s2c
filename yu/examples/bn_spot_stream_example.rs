/// WebSocket 发送订阅消息的示例
///
/// 这个示例展示如何：
/// 1. 连接到币安 WebSocket 组合流端点
/// 2. 发送 SUBSCRIBE 方法的 JSON 消消息来订阅多个交易对流
/// 3. 处理订阅成功/失败的响应
/// 4. 定时发送订阅和取消订阅请求
use actix::{Actor, Context, Handler};
use li::tools::logs::setup_logger;
use log::{info, LevelFilter};
use serde_json::to_string;
use std::collections::HashMap;
use yu::binance::jobs::initial_tables;
use yu::config::get_config;
use yu::duck_db::DBProvider;
use yu::websocket::subscribers::SpotStreamStorageActor;
use yue::binance::bn_json_websocket::{StreamCommandRequest, SPOT_STREAM_WEBSOCKET, WS_SUBSCRIBE_COMMAND};
use yue::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamResponse;
use yue::binance::websocket_handler::BinanceSpotStreamHandler;
use yue::websocket::client::{SendTextMessage, SubscribeToEvents, WebSocketClient, WebSocketEvent};
use yue::websocket::event_bus::{Subscribe, WsMessageBus};

/// 处理订阅消息的事件处理器
pub struct PrintSubscriberActor;

impl Actor for PrintSubscriberActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("PrintSubscriberActor started");
    }
}

impl Handler<BinanceSpotWebSocketStreamResponse> for PrintSubscriberActor {
    type Result = ();

    fn handle(&mut self, msg: BinanceSpotWebSocketStreamResponse, _ctx: &mut Context<Self>) {
        match msg {
            BinanceSpotWebSocketStreamResponse::Trade(trade) => {
                info!("PrintSubscriber received trade: {:?}", trade);
            }

            BinanceSpotWebSocketStreamResponse::Kline(kline) => {
                info!("PrintSubscriber received kline: {:?}", kline);
            }
            BinanceSpotWebSocketStreamResponse::PartialDepth(partial_depth) => {
                info!("PrintSubscriber received Partial Depth: {:?}", partial_depth);
            }
            BinanceSpotWebSocketStreamResponse::AggTrade(agg_trade) => {
                info!("PrintSubscriber received agg trade: {:?}", agg_trade);
            }
            BinanceSpotWebSocketStreamResponse::BookTicker(book_ticker) => {
                info!("PrintSubscriber received book ticker: {:?}", book_ticker);
            }
            BinanceSpotWebSocketStreamResponse::DepthUpdate(depth) => {
                info!("PrintSubscriber received update depth: {:?}", depth);
            }
        }
    }
}

#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    let mut special_log = HashMap::new();
    special_log.insert("yu".to_string(), LevelFilter::Trace);
    special_log.insert("li".to_string(), LevelFilter::Trace);
    special_log.insert("yue".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Warn), special_log).expect("TODO: panic message");

    info!("========== WebSocket 订阅消息示例 ==========");
    info!("本示例展示如何通过 WebSocket 发送 SUBSCRIBE/UNSUBSCRIBE 消息");
    info!("连接到币安现货 JSON RPC WebSocket API: {}", SPOT_STREAM_WEBSOCKET);

    // 步骤 1: 创建 WebSocket 客户端
    // 连接到币安现货 WebSocket JSON RPC 端点
    let client_addr = WebSocketClient::new(SPOT_STREAM_WEBSOCKET)
        .with_reconnect_interval(std::time::Duration::from_secs(5))
        .with_proxy("http://127.0.0.1:7891")
        .start();

    info!("✓ WebSocket 客户端已启动");
    if let Err(e) = initial_tables(None) {
        panic!("初始化数据库表失败: {:?}", e);
    }
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
    // 步骤 2: 创建事件处理器
    let bus = WsMessageBus::new(BinanceSpotStreamHandler {}).start();
    info!("✓ WsMessageBus started");
    let printer = PrintSubscriberActor.start();
    let storage_actor = SpotStreamStorageActor::new(spot_config.clone(), DBProvider::default()).start();

    bus.do_send(Subscribe {
        subscriber: printer.clone().recipient(),
    });
    bus.do_send(Subscribe {
        subscriber: storage_actor.clone().recipient(),
    });

    info!("✓ 事件处理器已启动");

    // 步骤 3: 订阅 WebSocket 事件
    client_addr
        .send(SubscribeToEvents {
            recipient: bus.recipient::<WebSocketEvent>(),
        })
        .await??;

    info!("✓ 事件处理器已订阅");

    // 等待连接建立和自动发送的订阅消息
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // 步骤 5: 发送第二个请求 - 获取账户信息
    info!("\n📤 发送请求 #2 - 订阅 BTCUSDT 和 ETHUSDT 的交易数据");
    let command_request = StreamCommandRequest {
        method: WS_SUBSCRIBE_COMMAND.to_string(),
        params: vec![
            // "btcusdt@trade".to_string(),
            // "btcusdt@bookTicker".to_string(),
            "ethusdt@kline_1m".to_string(),
            // "ethusdt@depth@100ms".to_string(),
        ],
        id: 0,
    };
    client_addr.send(SendTextMessage::new(to_string(&command_request).unwrap())).await??;

    info!("✓ 请求 #2 已发送");

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(90)).await;
    }

    info!("\n========== 示例结束 ==========");
    info!("提示: SPOT_WEBSOCKET 支持的方法包括:");
    info!("  - trades: 成交查询");
    info!("更多方法见: https://binance-docs.github.io/apidocs/spot/cn/");

    Ok(())
}
