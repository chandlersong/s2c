use yue::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamResponse;

#[test]
fn test_parse_trade_stream() {
    let json = r#"{
        "e": "trade",
        "E": 1514035000000,
        "s": "BTCUSDT",
        "t": 12345,
        "p": "25100.00",
        "q": "1.00",
        "T": 1514035000000,
        "m": false,
        "M": true
    }"#;

    let result = BinanceSpotWebSocketStreamResponse::from_text(json);
    assert!(result.is_ok(), "应该能成功解析逐笔交易消息");

    match result.unwrap() {
        BinanceSpotWebSocketStreamResponse::Trade(payload) => {
            assert_eq!(payload.symbol, "BTCUSDT");
            assert_eq!(payload.trade_id, 12345);
            assert_eq!(payload.price, 25100.00);
            assert_eq!(payload.qty, 1.00);
        }
        _ => panic!("应该解析为 Trade 变体"),
    }
}

#[test]
fn test_parse_agg_trade_stream() {
    let json = r#"{
        "e": "aggTrade",
        "E": 1514035000000,
        "s": "BTCUSDT",
        "a": 12345,
        "p": "25100.00",
        "q": "1.00",
        "f": 100,
        "l": 200,
        "T": 1514035000000,
        "m": false
    }"#;

    let result = BinanceSpotWebSocketStreamResponse::from_text(json);
    assert!(result.is_ok(), "应该能成功解析归集交易消息");

    match result.unwrap() {
        BinanceSpotWebSocketStreamResponse::AggTrade(payload) => {
            assert_eq!(payload.symbol, "BTCUSDT");
            assert_eq!(payload.agg_id, 12345);
            assert_eq!(payload.price, 25100.00);
            assert_eq!(payload.first_trade_id, 100);
            assert_eq!(payload.last_trade_id, 200);
        }
        _ => panic!("应该解析为 AggTrade 变体"),
    }
}

#[test]
fn test_parse_kline_stream() {
    let json = r#"{
        "e": "kline",
        "E": 1514035000000,
        "s": "BTCUSDT",
        "k": {
            "t": 1514035000000,
            "T": 1514035060000,
            "s": "BTCUSDT",
            "i": "1m",
            "f": 100,
            "L": 200,
            "o": "25100.00",
            "c": "25200.00",
            "h": "25300.00",
            "l": "25000.00",
            "v": "100.00",
            "n": 50,
            "x": true,
            "q": "2500000.00",
            "V": "50.00",
            "Q": "1250000.00"
        }
    }"#;

    let result = BinanceSpotWebSocketStreamResponse::from_text(json);
    assert!(result.is_ok(), "应该能成功解析K线消息");

    match result.unwrap() {
        BinanceSpotWebSocketStreamResponse::Kline(payload) => {
            assert_eq!(payload.symbol, "BTCUSDT");
            assert_eq!(payload.kline.interval, "1m");
            assert_eq!(payload.kline.open, 25100.00);
            assert_eq!(payload.kline.close, 25200.00);
            assert!(payload.kline.is_closed);
        }
        _ => panic!("应该解析为 Kline 变体"),
    }
}

#[test]
fn test_parse_partial_depth_stream() {
    let json = r#"{
        "lastUpdateId": 160943312,
        "bids": [["25100.00", "1.00"], ["25099.00", "2.00"]],
        "asks": [["25200.00", "2.00"], ["25201.00", "1.00"]]
    }"#;

    let result = BinanceSpotWebSocketStreamResponse::from_text(json);
    assert!(result.is_ok(), "应该能成功解析有限档深度消息");

    match result.unwrap() {
        BinanceSpotWebSocketStreamResponse::PartialDepth(payload) => {
            assert_eq!(payload.last_update_id, 160943312);
            assert_eq!(payload.bids.len(), 2);
            assert_eq!(payload.asks.len(), 2);
            assert_eq!(payload.bids[0].0, 25100.00);
            assert_eq!(payload.asks[0].0, 25200.00);
        }
        _ => panic!("应该解析为 PartialDepth 变体"),
    }
}

#[test]
fn test_parse_book_ticker_stream() {
    let json = r#"{
        "u": 400900217,
        "s": "BTCUSDT",
        "b": "25100.00",
        "B": "1.00",
        "a": "25200.00",
        "A": "2.00"
    }"#;

    let result = BinanceSpotWebSocketStreamResponse::from_text(json);
    assert!(result.is_ok(), "应该能成功解析按Symbol的最优挂单消息");

    match result.unwrap() {
        BinanceSpotWebSocketStreamResponse::BookTicker(payload) => {
            assert_eq!(payload.symbol, "BTCUSDT");
            assert_eq!(payload.best_bid_price, 25100.00);
            assert_eq!(payload.best_ask_price, 25200.00);
            assert_eq!(payload.update_id, 400900217);
        }
        _ => panic!("应该解析为 BookTicker 变体"),
    }
}

#[test]
fn test_parse_multiple_stream_types() {
    let messages = vec![
        (
            r#"{"e":"trade","E":1514035000000,"s":"BTCUSDT","t":12345,"p":"25100.00","q":"1.00","b":111,"a":222,"T":1514035000000,"m":false}"#,
            "Trade",
        ),
        (
            r#"{"e":"aggTrade","E":1514035000000,"s":"BTCUSDT","a":12345,"p":"25100.00","q":"1.00","f":100,"l":200,"T":1514035000000,"m":false}"#,
            "AggTrade",
        ),
        (
            r#"{"e":"kline","E":1514035000000,"s":"BTCUSDT","k":{"t":1514035000000,"T":1514035060000,"s":"BTCUSDT","i":"1m","f":100,"L":200,"o":"25100.00","c":"25200.00","h":"25300.00","l":"25000.00","v":"100.00","n":50,"x":true,"q":"2500000.00","V":"50.00","Q":"1250000.00"}}"#,
            "Kline",
        ),
        (
            r#"{"lastUpdateId":160943312,"bids":[["25100.00","1.00"]],"asks":[["25200.00","2.00"]]}"#,
            "PartialDepth",
        ),
        (
            r#"{"u":400900217,"s":"BTCUSDT","b":"25100.00","B":"1.00","a":"25200.00","A":"2.00"}"#,
            "BookTicker",
        ),
    ];

    for (json, expected_type) in messages {
        let result = BinanceSpotWebSocketStreamResponse::from_text(json);
        assert!(result.is_ok(), "应该能成功解析 {} 消息", expected_type);

        let response = result.unwrap();
        match expected_type {
            "Trade" => {
                assert!(matches!(response, BinanceSpotWebSocketStreamResponse::Trade(_)), "应该识别为 Trade");
            }
            "AggTrade" => {
                assert!(matches!(response, BinanceSpotWebSocketStreamResponse::AggTrade(_)), "应该识别为 AggTrade");
            }
            "Kline" => {
                assert!(matches!(response, BinanceSpotWebSocketStreamResponse::Kline(_)), "应该识别为 Kline");
            }
            "PartialDepth" => {
                assert!(
                    matches!(response, BinanceSpotWebSocketStreamResponse::PartialDepth(_)),
                    "应该识别为 PartialDepth"
                );
            }
            "BookTicker" => {
                assert!(
                    matches!(response, BinanceSpotWebSocketStreamResponse::BookTicker(_)),
                    "应该识别为 BookTicker"
                );
            }
            _ => panic!("未知的类型"),
        }
    }
}
