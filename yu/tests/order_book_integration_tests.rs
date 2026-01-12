// 订单簿维护模块的集成测试

#[cfg(test)]
mod order_book_integration_tests {
    use rust_decimal::Decimal;
    use yu::order_book::{
        order_book::{DepthEvent, MarketType, Side, Snapshot},
        Synchronizer,
    };

    #[test]
    fn test_complete_workflow() {
        // 测试完整的工作流：快照 -> 事件应用 -> 缺口检测 -> 重同步

        let mut sync = Synchronizer::new("BTCUSDT".to_string(), MarketType::Spot, 100);

        // 阶段 1：应用快照
        let snapshot = Snapshot {
            last_update_id: 1000,
            bids: vec![
                (Decimal::from_str_exact("50000").unwrap(), Decimal::from(10)),
                (Decimal::from_str_exact("49999").unwrap(), Decimal::from(20)),
            ],
            asks: vec![
                (Decimal::from_str_exact("50001").unwrap(), Decimal::from(10)),
                (Decimal::from_str_exact("50002").unwrap(), Decimal::from(20)),
            ],
        };

        sync.init(&snapshot).unwrap();
        assert_eq!(sync.local_update_id(), 1000);
        assert_eq!(sync.order_book.size(), 4);

        // 阶段 2：应用正常事件
        let event1 = DepthEvent {
            first_update_id: 1001,
            final_update_id: 1001,
            event_time: 2000,
            bids: vec![(Decimal::from_str_exact("50000").unwrap(), Decimal::from(15))],
            asks: vec![],
        };

        let needs_resync = sync.on_event(event1).unwrap();
        assert!(!needs_resync);
        assert_eq!(sync.local_update_id(), 1001);

        // 阶段 3：应用缺口事件
        let event_gap = DepthEvent {
            first_update_id: 1003, // 缺口！应该是 1002
            final_update_id: 1003,
            event_time: 2001,
            bids: vec![],
            asks: vec![],
        };

        let needs_resync = sync.on_event(event_gap).unwrap();
        assert!(needs_resync); // 返回需要重同步

        // 阶段 4：重同步
        sync.trigger_resync().unwrap();
        let stats = sync.get_stats();
        assert_eq!(stats.resync_count, 1);

        // 重新初始化
        sync.init(&snapshot).unwrap();
        assert_eq!(sync.local_update_id(), 1000);
    }

    #[test]
    fn test_event_replay_after_snapshot() {
        // 测试快照后事件应用（事件需要在初始化前缓存）
        // 通过先应用快照再应用事件来验证

        let mut sync = Synchronizer::new("ETHUSDT".to_string(), MarketType::Spot, 100);

        let snapshot = Snapshot {
            last_update_id: 1000,
            bids: vec![(Decimal::from_str_exact("3001").unwrap(), Decimal::from(1))],
            asks: vec![],
        };

        sync.init(&snapshot).unwrap();
        assert_eq!(sync.local_update_id(), 1000);

        // 现在应用一些事件
        let event1 = DepthEvent {
            first_update_id: 1001,
            final_update_id: 1001,
            event_time: 2000,
            bids: vec![(Decimal::from_str_exact("3000").unwrap(), Decimal::from(5))],
            asks: vec![],
        };

        sync.on_event(event1).unwrap();

        let event2 = DepthEvent {
            first_update_id: 1002,
            final_update_id: 1002,
            event_time: 2001,
            bids: vec![(Decimal::from_str_exact("2999").unwrap(), Decimal::from(10))],
            asks: vec![],
        };

        sync.on_event(event2).unwrap();

        // 验证所有事件都被应用
        assert_eq!(sync.local_update_id(), 1002);
        assert_eq!(sync.order_book.size(), 3); // 1000 的快照 + 1001 和 1002 的事件
    }

    #[test]
    fn test_export_top_functionality() {
        // 测试导出顶部档位

        let mut sync = Synchronizer::new("BNBUSDT".to_string(), MarketType::Spot, 100);

        let snapshot = Snapshot {
            last_update_id: 500,
            bids: vec![
                (Decimal::from(500), Decimal::from(100)),
                (Decimal::from(499), Decimal::from(200)),
                (Decimal::from(498), Decimal::from(300)),
                (Decimal::from(497), Decimal::from(400)),
            ],
            asks: vec![
                (Decimal::from(501), Decimal::from(100)),
                (Decimal::from(502), Decimal::from(200)),
                (Decimal::from(503), Decimal::from(300)),
                (Decimal::from(504), Decimal::from(400)),
            ],
        };

        sync.init(&snapshot).unwrap();

        // 导出前 2 档买盘
        let bid_top = sync.order_book.export_top(Side::Bid, 2);
        assert_eq!(bid_top.entries.len(), 2);
        assert_eq!(bid_top.entries[0].price, Decimal::from(500));
        assert_eq!(bid_top.entries[1].price, Decimal::from(499));
        assert_eq!(bid_top.symbol, "BNBUSDT");

        // 导出前 2 档卖盘
        let ask_top = sync.order_book.export_top(Side::Ask, 2);
        assert_eq!(ask_top.entries.len(), 2);
        assert_eq!(ask_top.entries[0].price, Decimal::from(501));
        assert_eq!(ask_top.entries[1].price, Decimal::from(502));

        // 导出两端
        let both_top = sync.order_book.export_top(Side::Both, 2);
        assert_eq!(both_top.entries.len(), 4); // 2 档买 + 2 档卖
    }

    #[test]
    fn test_cache_overflow() {
        // 测试缓存溢出

        let mut sync = Synchronizer::new("LTCUSDT".to_string(), MarketType::Spot, 5);

        let snapshot = Snapshot {
            last_update_id: 100,
            bids: vec![],
            asks: vec![],
        };

        sync.init(&snapshot).unwrap();

        // 添加 5 个事件
        for i in 1..=5 {
            let event = DepthEvent {
                first_update_id: 100 + i,
                final_update_id: 100 + i,
                event_time: 2000,
                bids: vec![],
                asks: vec![],
            };
            let _ = sync.on_event(event).unwrap();
        }

        assert_eq!(sync.cache_len(), 5);

        // 第 6 个事件应该导致缓存溢出
        let event = DepthEvent {
            first_update_id: 106,
            final_update_id: 106,
            event_time: 2000,
            bids: vec![],
            asks: vec![],
        };

        let result = sync.on_event(event);
        assert!(result.is_err()); // 应该返回错误
        assert_eq!(sync.cache_len(), 0); // 缓存应该已清空
    }

    #[test]
    fn test_delete_level_with_zero_qty() {
        // 测试删除数量为 0 的价位

        let mut sync = Synchronizer::new("XRPUSDT".to_string(), MarketType::Spot, 100);

        let snapshot = Snapshot {
            last_update_id: 50,
            bids: vec![(Decimal::from_str_exact("1.5").unwrap(), Decimal::from(1000))],
            asks: vec![(Decimal::from_str_exact("1.6").unwrap(), Decimal::from(1000))],
        };

        sync.init(&snapshot).unwrap();
        assert_eq!(sync.order_book.size(), 2);

        // 删除买盘
        let event = DepthEvent {
            first_update_id: 51,
            final_update_id: 51,
            event_time: 3000,
            bids: vec![(Decimal::from_str_exact("1.5").unwrap(), Decimal::ZERO)],
            asks: vec![],
        };

        sync.on_event(event).unwrap();
        assert_eq!(sync.order_book.size(), 1);
    }

    #[test]
    fn test_stats_tracking() {
        // 测试统计信息跟踪

        let mut sync = Synchronizer::new("DOGEUSDT".to_string(), MarketType::Spot, 100);

        let snapshot = Snapshot {
            last_update_id: 200,
            bids: vec![],
            asks: vec![],
        };

        sync.init(&snapshot).unwrap();

        // 应用多个事件
        for i in 1..=5 {
            let event = DepthEvent {
                first_update_id: 200 + i,
                final_update_id: 200 + i,
                event_time: 5000,
                bids: vec![],
                asks: vec![],
            };
            let _ = sync.on_event(event);
        }

        let stats = sync.get_stats();
        assert_eq!(stats.processed_events, 5);
        assert_eq!(stats.cache_size, 5);
        assert_eq!(stats.reconnect_count, 0);

        // 记录重连
        sync.record_reconnect();
        let stats = sync.get_stats();
        assert_eq!(stats.reconnect_count, 1);
    }
}
