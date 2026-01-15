use actix::prelude::*;
use rust_decimal::Decimal;
use std::sync::Arc;
use std::time::Duration;
use yu::binance::bn_dashboard::MarketDepthDashBoard;
use yue::binance::bn_models::spot_restful::Depth;
use yue::binance::order_book::{OrderBook, OrderBookSnapshotMsg};

#[actix::test]
async fn test_market_depth_dashboard_creation() {
    let dashboard = MarketDepthDashBoard::new().start();
    assert!(dashboard.connected());
}

#[actix::test]
async fn test_receive_order_book_snapshot() {
    let dashboard = MarketDepthDashBoard::new().start();

    let depth = Depth {
        last_update_id: 100,
        bids: vec![(Decimal::new(50000, 0), Decimal::new(1, 0)), (Decimal::new(49999, 0), Decimal::new(2, 0))],
        asks: vec![(Decimal::new(50001, 0), Decimal::new(1, 0)), (Decimal::new(50002, 0), Decimal::new(2, 0))],
    };

    let order_book = OrderBook::new("BTCUSDT", depth).unwrap();
    let msg = OrderBookSnapshotMsg(Arc::new(order_book));

    dashboard.do_send(msg);

    tokio::time::sleep(Duration::from_millis(100)).await;

    let result = dashboard
        .send(yu::binance::bn_dashboard::QueryDepth {
            symbol: "BTCUSDT".to_string(),
        })
        .await;
    assert!(result.is_ok());
    let depth_result = result.unwrap();
    assert!(depth_result.is_some());
    let ob = depth_result.unwrap();
    assert_eq!(ob.symbol, "BTCUSDT");
    assert_eq!(ob.local_update_id, 100);
}

#[actix::test]
async fn test_query_nonexistent_symbol() {
    let dashboard = MarketDepthDashBoard::new().start();

    let result = dashboard
        .send(yu::binance::bn_dashboard::QueryDepth {
            symbol: "ETHUSDT".to_string(),
        })
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[actix::test]
async fn test_query_all_symbols() {
    let dashboard = MarketDepthDashBoard::new().start();

    let depth1 = Depth {
        last_update_id: 100,
        bids: vec![(Decimal::new(50000, 0), Decimal::new(1, 0))],
        asks: vec![(Decimal::new(50001, 0), Decimal::new(1, 0))],
    };
    let depth2 = Depth {
        last_update_id: 200,
        bids: vec![(Decimal::new(3000, 0), Decimal::new(10, 0))],
        asks: vec![(Decimal::new(3001, 0), Decimal::new(10, 0))],
    };

    let ob1 = OrderBook::new("BTCUSDT", depth1).unwrap();
    let ob2 = OrderBook::new("ETHUSDT", depth2).unwrap();

    dashboard.do_send(OrderBookSnapshotMsg(Arc::new(ob1)));
    dashboard.do_send(OrderBookSnapshotMsg(Arc::new(ob2)));

    tokio::time::sleep(Duration::from_millis(100)).await;

    let result = dashboard.send(yu::binance::bn_dashboard::QueryAllSymbols).await;
    assert!(result.is_ok());
    let symbols = result.unwrap();
    assert_eq!(symbols.len(), 2);
    assert!(symbols.contains(&"BTCUSDT".to_string()));
    assert!(symbols.contains(&"ETHUSDT".to_string()));
}

#[actix::test]
async fn test_update_existing_symbol() {
    let dashboard = MarketDepthDashBoard::new().start();

    let depth1 = Depth {
        last_update_id: 100,
        bids: vec![(Decimal::new(50000, 0), Decimal::new(1, 0))],
        asks: vec![(Decimal::new(50001, 0), Decimal::new(1, 0))],
    };

    let ob1 = OrderBook::new("BTCUSDT", depth1).unwrap();
    dashboard.do_send(OrderBookSnapshotMsg(Arc::new(ob1)));

    tokio::time::sleep(Duration::from_millis(100)).await;

    let depth2 = Depth {
        last_update_id: 200,
        bids: vec![(Decimal::new(51000, 0), Decimal::new(2, 0))],
        asks: vec![(Decimal::new(51001, 0), Decimal::new(2, 0))],
    };

    let ob2 = OrderBook::new("BTCUSDT", depth2).unwrap();
    dashboard.do_send(OrderBookSnapshotMsg(Arc::new(ob2)));

    tokio::time::sleep(Duration::from_millis(100)).await;

    let result = dashboard
        .send(yu::binance::bn_dashboard::QueryDepth {
            symbol: "BTCUSDT".to_string(),
        })
        .await;
    assert!(result.is_ok());
    let depth_result = result.unwrap();
    assert!(depth_result.is_some());
    let ob = depth_result.unwrap();
    assert_eq!(ob.local_update_id, 200);
}

#[actix::test]
async fn test_batch_query_depths() {
    let dashboard = MarketDepthDashBoard::new().start();

    let depth1 = Depth {
        last_update_id: 100,
        bids: vec![(Decimal::new(50000, 0), Decimal::new(1, 0))],
        asks: vec![(Decimal::new(50001, 0), Decimal::new(1, 0))],
    };
    let depth2 = Depth {
        last_update_id: 200,
        bids: vec![(Decimal::new(3000, 0), Decimal::new(10, 0))],
        asks: vec![(Decimal::new(3001, 0), Decimal::new(10, 0))],
    };

    let ob1 = OrderBook::new("BTCUSDT", depth1).unwrap();
    let ob2 = OrderBook::new("ETHUSDT", depth2).unwrap();

    dashboard.do_send(OrderBookSnapshotMsg(Arc::new(ob1)));
    dashboard.do_send(OrderBookSnapshotMsg(Arc::new(ob2)));

    tokio::time::sleep(Duration::from_millis(100)).await;

    let result = dashboard
        .send(yu::binance::bn_dashboard::QueryBatchDepths {
            symbols: vec!["BTCUSDT".to_string(), "ETHUSDT".to_string(), "BNBUSDT".to_string()],
        })
        .await;
    assert!(result.is_ok());
    let depths = result.unwrap();
    assert_eq!(depths.len(), 2);
    assert!(depths.iter().any(|ob| ob.symbol == "BTCUSDT"));
    assert!(depths.iter().any(|ob| ob.symbol == "ETHUSDT"));
}
