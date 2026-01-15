use actix::prelude::*;
use li::tools::logs::setup_logger;
use log::LevelFilter;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use yu::binance::bn_dashboard::{MarketDepthDashBoard, QueryAllSymbols, QueryBatchDepths, QueryDepth};
use yue::binance::bn_models::spot_restful::Depth;
use yue::binance::order_book::{OrderBook, OrderBookSnapshotMsg};

#[actix::main]
async fn main() {
    let mut special_log = HashMap::new();
    special_log.insert("yue".to_string(), LevelFilter::Info);
    special_log.insert("li".to_string(), LevelFilter::Info);
    setup_logger(Some(LevelFilter::Warn), special_log).expect("日志初始化失败");

    println!("========== MarketDepthDashBoard 示例 ==========");

    println!("========== MarketDepthDashBoard 示例 ==========");

    let dashboard = MarketDepthDashBoard::new().start();
    println!("MarketDepthDashBoard 已启动");

    println!("\n1. 模拟接收BTCUSDT的订单簿快照");
    let btc_depth = Depth {
        last_update_id: 100,
        bids: vec![
            (Decimal::new(50000, 0), Decimal::new(1, 0)),
            (Decimal::new(49999, 0), Decimal::new(2, 0)),
            (Decimal::new(49998, 0), Decimal::new(3, 0)),
        ],
        asks: vec![
            (Decimal::new(50001, 0), Decimal::new(1, 0)),
            (Decimal::new(50002, 0), Decimal::new(2, 0)),
            (Decimal::new(50003, 0), Decimal::new(3, 0)),
        ],
    };
    let btc_ob = OrderBook::new("BTCUSDT", btc_depth).unwrap();
    dashboard.do_send(OrderBookSnapshotMsg(Arc::new(btc_ob)));
    tokio::time::sleep(Duration::from_millis(100)).await;

    println!("\n2. 模拟接收ETHUSDT的订单簿快照");
    let eth_depth = Depth {
        last_update_id: 200,
        bids: vec![(Decimal::new(3000, 0), Decimal::new(10, 0)), (Decimal::new(2999, 0), Decimal::new(20, 0))],
        asks: vec![(Decimal::new(3001, 0), Decimal::new(10, 0)), (Decimal::new(3002, 0), Decimal::new(20, 0))],
    };
    let eth_ob = OrderBook::new("ETHUSDT", eth_depth).unwrap();
    dashboard.do_send(OrderBookSnapshotMsg(Arc::new(eth_ob)));
    tokio::time::sleep(Duration::from_millis(100)).await;

    println!("\n3. 查询所有symbol列表");
    let symbols = dashboard.send(QueryAllSymbols).await.unwrap();
    println!("当前缓存的symbols: {:?}", symbols);

    println!("\n4. 查询BTCUSDT的订单簿");
    if let Some(ob) = dashboard
        .send(QueryDepth {
            symbol: "BTCUSDT".to_string(),
        })
        .await
        .unwrap()
    {
        println!("BTCUSDT 订单簿:");
        println!("  Symbol: {}", ob.symbol);
        println!("  Update ID: {}", ob.local_update_id);
        println!("  Update Time: {}", ob.last_update_time);
    }

    println!("\n5. 批量查询多个symbol的订单簿");
    let depths = dashboard
        .send(QueryBatchDepths {
            symbols: vec!["BTCUSDT".to_string(), "ETHUSDT".to_string(), "BNBUSDT".to_string()],
        })
        .await
        .unwrap();
    println!("批量查询结果，共获取到 {} 个订单簿:", depths.len());
    for ob in depths {
        println!("  - {}: update_id={}", ob.symbol, ob.local_update_id);
    }

    println!("\n6. 查询不存在的symbol");
    let result = dashboard
        .send(QueryDepth {
            symbol: "NOTEXIST".to_string(),
        })
        .await
        .unwrap();
    if result.is_none() {
        println!("NOTEXIST 未找到，返回None");
    }

    println!("\n7. 更新BTCUSDT的订单簿");
    let btc_depth_new = Depth {
        last_update_id: 300,
        bids: vec![
            (Decimal::new(51000, 0), Decimal::new(5, 0)),
            (Decimal::new(50999, 0), Decimal::new(10, 0)),
        ],
        asks: vec![
            (Decimal::new(51001, 0), Decimal::new(5, 0)),
            (Decimal::new(51002, 0), Decimal::new(10, 0)),
        ],
    };
    let btc_ob_new = OrderBook::new("BTCUSDT", btc_depth_new).unwrap();
    dashboard.do_send(OrderBookSnapshotMsg(Arc::new(btc_ob_new)));
    tokio::time::sleep(Duration::from_millis(100)).await;

    println!("\n8. 再次查询BTCUSDT验证更新");
    if let Some(ob) = dashboard
        .send(QueryDepth {
            symbol: "BTCUSDT".to_string(),
        })
        .await
        .unwrap()
    {
        println!("BTCUSDT 订单簿已更新:");
        println!("  Update ID: {}", ob.local_update_id);
    }

    println!("\n========== 示例结束 ==========");
}
