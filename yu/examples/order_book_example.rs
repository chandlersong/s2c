// 订单簿维护和同步管理器集成示例
// 演示如何将 OrderBook 和 Synchronizer 一起使用

use rust_decimal::Decimal;
use yu::order_book::{
    order_book::{DepthEvent, MarketType, Snapshot},
    Synchronizer,
};

fn main() {
    println!("=== 订单簿维护集成示例 ===\n");

    // 创建同步管理器（BTCUSDT 现货，最多缓存 1000 个事件）
    let mut sync = Synchronizer::new("BTCUSDT".to_string(), MarketType::Spot, 1000);

    println!("1. 初始化快照");
    let snapshot = Snapshot {
        last_update_id: 100,
        bids: vec![
            (Decimal::from_str_exact("50000").unwrap(), Decimal::from(1)),
            (Decimal::from_str_exact("49999").unwrap(), Decimal::from(2)),
            (Decimal::from_str_exact("49998").unwrap(), Decimal::from(3)),
        ],
        asks: vec![
            (Decimal::from_str_exact("50001").unwrap(), Decimal::from(1)),
            (Decimal::from_str_exact("50002").unwrap(), Decimal::from(2)),
            (Decimal::from_str_exact("50003").unwrap(), Decimal::from(3)),
        ],
    };

    sync.init(&snapshot).unwrap();
    println!("✓ 快照应用成功");
    println!("  本地 ID: {}", sync.local_update_id());
    println!("  订单簿大小: {}", sync.order_book.size());
    println!("  状态: {}\n", sync.state());

    // 应用一些增量事件
    println!("2. 应用增量事件");
    let events = vec![
        DepthEvent {
            first_update_id: 101,
            final_update_id: 101,
            event_time: 1000,
            bids: vec![(Decimal::from_str_exact("50000").unwrap(), Decimal::from(5))],
            asks: vec![],
        },
        DepthEvent {
            first_update_id: 102,
            final_update_id: 102,
            event_time: 1001,
            bids: vec![(Decimal::from_str_exact("49999").unwrap(), Decimal::ZERO)],
            asks: vec![(Decimal::from_str_exact("50004").unwrap(), Decimal::from(1))],
        },
    ];

    for event in events {
        match sync.on_event(event) {
            Ok(needs_resync) => {
                if needs_resync {
                    println!("⚠ 检测到缺口，需要重同步");
                } else {
                    println!("✓ 事件应用成功");
                }
            }
            Err(e) => {
                println!("✗ 事件处理失败: {}", e);
            }
        }
    }

    println!("  本地 ID: {}", sync.local_update_id());
    println!("  订单簿大小: {}", sync.order_book.size());
    println!("  缓存大小: {}\n", sync.cache_len());

    // 导出顶部档位
    println!("3. 导出顶部档位");
    let bid_view = sync.order_book.export_top(yu::order_book::order_book::Side::Bid, 2);
    println!("买盘前 2 档:");
    for entry in &bid_view.entries {
        println!("  #{}: {} @ {}", entry.level, entry.qty, entry.price);
    }

    let ask_view = sync.order_book.export_top(yu::order_book::order_book::Side::Ask, 2);
    println!("卖盘前 2 档:");
    for entry in &ask_view.entries {
        println!("  #{}: {} @ {}", entry.level, entry.qty, entry.price);
    }

    // 查看统计信息
    println!("\n4. 统计信息");
    let stats = sync.get_stats();
    println!("  重连次数: {}", stats.reconnect_count);
    println!("  重同步次数: {}", stats.resync_count);
    println!("  处理事件数: {}", stats.processed_events);
    println!("  缓存大小: {}", stats.cache_size);
    println!("  平均延迟: {:.2} ms", stats.avg_process_delay_ms);

    println!("\n=== 示例完成 ===");
}
