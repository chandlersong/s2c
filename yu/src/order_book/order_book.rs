use crate::errors::YuError;
use rust_decimal::Decimal;
use std::collections::BTreeMap;

/// 订单簿端类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Bid,
    Ask,
    Both,
}

/// 事件应用结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyResult {
    /// 事件成功应用
    Success,
    /// 事件已过期（u < local_update_id）
    Skipped,
    /// 检测到缺口（U > local_update_id + 1）
    GapDetected,
}

/// 市场类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketType {
    Spot,
    Swap,
}

impl std::fmt::Display for MarketType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarketType::Spot => write!(f, "spot"),
            MarketType::Swap => write!(f, "swap"),
        }
    }
}

/// 深度事件表示
#[derive(Debug, Clone)]
pub struct DepthEvent {
    /// 首个更新 ID（对应 websocket 的 U）
    pub first_update_id: u64,
    /// 最终更新 ID（对应 websocket 的 u）
    pub final_update_id: u64,
    /// 事件时间（毫秒）
    pub event_time: u64,
    /// 买盘增量 [(price, qty)]
    pub bids: Vec<(Decimal, Decimal)>,
    /// 卖盘增量 [(price, qty)]
    pub asks: Vec<(Decimal, Decimal)>,
}

/// 深度快照表示
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// 快照对应的最后更新 ID
    pub last_update_id: u64,
    /// 买盘 [(price, qty)]
    pub bids: Vec<(Decimal, Decimal)>,
    /// 卖盘 [(price, qty)]
    pub asks: Vec<(Decimal, Decimal)>,
}

/// 导出顶部档位条目
#[derive(Debug, Clone)]
pub struct TopEntry {
    /// 价位
    pub price: Decimal,
    /// 数量
    pub qty: Decimal,
    /// 档位（从 1 开始）
    pub level: usize,
}

/// 导出的顶部档位视图
#[derive(Debug, Clone)]
pub struct TopView {
    /// 交易对
    pub symbol: String,
    /// 市场类型
    pub market_type: MarketType,
    /// 端类型
    pub side: Side,
    /// 顶部档位条目
    pub entries: Vec<TopEntry>,
    /// 对应的更新 ID
    pub update_id: u64,
    /// 时间戳（毫秒）
    pub ts: u64,
}

/// 本地订单簿维护结构
#[derive(Debug, Clone)]
pub struct OrderBook {
    /// 交易对
    pub symbol: String,
    /// 市场类型
    pub market_type: MarketType,
    /// 买盘：价位 -> 数量（BTreeMap 自动按价位排序）
    bids: BTreeMap<Decimal, Decimal>,
    /// 卖盘：价位 -> 数量
    asks: BTreeMap<Decimal, Decimal>,
    /// 本地更新 ID（最后应用的事件的 u）
    pub local_update_id: u64,
    /// 最后更新时间戳（毫秒）
    pub last_update_time: u64,
}

impl OrderBook {
    /// 创建新的订单簿
    pub fn new(symbol: String, market_type: MarketType) -> Self {
        Self {
            symbol,
            market_type,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            local_update_id: 0,
            last_update_time: 0,
        }
    }

    /// 应用快照初始化订单簿
    pub fn apply_snapshot(&mut self, snapshot: &Snapshot) -> Result<(), YuError> {
        self.bids.clear();
        self.asks.clear();

        // 加载快照的买盘
        for (price, qty) in &snapshot.bids {
            if qty > &Decimal::ZERO {
                self.bids.insert(*price, *qty);
            }
        }

        // 加载快照的卖盘
        for (price, qty) in &snapshot.asks {
            if qty > &Decimal::ZERO {
                self.asks.insert(*price, *qty);
            }
        }

        self.local_update_id = snapshot.last_update_id;
        self.last_update_time = 0; // 快照时间需要由调用者提供（如果需要）

        Ok(())
    }

    /// 应用增量事件，返回应用结果
    pub fn apply_event(&mut self, ev: &DepthEvent) -> Result<ApplyResult, YuError> {
        // 检查过期事件
        if ev.final_update_id < self.local_update_id {
            return Ok(ApplyResult::Skipped);
        }

        // 检查缺口
        if ev.first_update_id > self.local_update_id + 1 {
            return Ok(ApplyResult::GapDetected);
        }

        // 应用买盘增量
        for (price, qty) in &ev.bids {
            if qty == &Decimal::ZERO {
                self.bids.remove(price);
            } else {
                self.bids.insert(*price, *qty);
            }
        }

        // 应用卖盘增量
        for (price, qty) in &ev.asks {
            if qty == &Decimal::ZERO {
                self.asks.remove(price);
            } else {
                self.asks.insert(*price, *qty);
            }
        }

        // 更新本地 ID 和时间戳
        self.local_update_id = ev.final_update_id;
        self.last_update_time = ev.event_time;

        Ok(ApplyResult::Success)
    }

    /// 导出前 n 档数据
    pub fn export_top(&self, side: Side, n: usize) -> TopView {
        let mut entries = Vec::new();

        match side {
            Side::Bid => {
                // 买盘：按价位降序排列（BTreeMap 存储的是升序，所以需要反向迭代）
                for (price, qty) in self.bids.iter().rev().take(n) {
                    entries.push(TopEntry {
                        price: *price,
                        qty: *qty,
                        level: entries.len() + 1,
                    });
                }
            }
            Side::Ask => {
                // 卖盘：按价位升序排列
                for (price, qty) in self.asks.iter().take(n) {
                    entries.push(TopEntry {
                        price: *price,
                        qty: *qty,
                        level: entries.len() + 1,
                    });
                }
            }
            Side::Both => {
                // 买盘（降序）
                let mut bid_entries: Vec<_> = self
                    .bids
                    .iter()
                    .rev()
                    .take(n)
                    .map(|(price, qty)| TopEntry {
                        price: *price,
                        qty: *qty,
                        level: 0, // 稍后会调整
                    })
                    .collect();

                // 卖盘（升序）
                let mut ask_entries: Vec<_> = self
                    .asks
                    .iter()
                    .take(n)
                    .map(|(price, qty)| TopEntry {
                        price: *price,
                        qty: *qty,
                        level: 0, // 稍后会调整
                    })
                    .collect();

                // 调整档位编号
                for (i, entry) in bid_entries.iter_mut().enumerate() {
                    entry.level = i + 1;
                }
                for (i, entry) in ask_entries.iter_mut().enumerate() {
                    entry.level = i + 1;
                }

                entries.extend(bid_entries);
                entries.extend(ask_entries);
            }
        }

        TopView {
            symbol: self.symbol.clone(),
            market_type: self.market_type,
            side,
            entries,
            update_id: self.local_update_id,
            ts: self.last_update_time,
        }
    }

    /// 获取当前订单簿大小（总档位数）
    pub fn size(&self) -> usize {
        self.bids.len() + self.asks.len()
    }

    /// 清空订单簿
    pub fn clear(&mut self) {
        self.bids.clear();
        self.asks.clear();
        self.local_update_id = 0;
        self.last_update_time = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_order_book() {
        let ob = OrderBook::new("BTCUSDT".to_string(), MarketType::Spot);
        assert_eq!(ob.symbol, "BTCUSDT");
        assert_eq!(ob.local_update_id, 0);
        assert_eq!(ob.size(), 0);
    }

    #[test]
    fn test_apply_snapshot() {
        let mut ob = OrderBook::new("BTCUSDT".to_string(), MarketType::Spot);

        let snapshot = Snapshot {
            last_update_id: 100,
            bids: vec![(Decimal::from(50000), Decimal::from(1)), (Decimal::from(49999), Decimal::from(2))],
            asks: vec![(Decimal::from(50001), Decimal::from(1)), (Decimal::from(50002), Decimal::from(2))],
        };

        ob.apply_snapshot(&snapshot).unwrap();
        assert_eq!(ob.local_update_id, 100);
        assert_eq!(ob.size(), 4);
    }

    #[test]
    fn test_apply_event_success() {
        let mut ob = OrderBook::new("BTCUSDT".to_string(), MarketType::Spot);
        ob.local_update_id = 100;

        let event = DepthEvent {
            first_update_id: 101,
            final_update_id: 101,
            event_time: 1000,
            bids: vec![(Decimal::from(50000), Decimal::from(5))],
            asks: vec![],
        };

        let result = ob.apply_event(&event).unwrap();
        assert_eq!(result, ApplyResult::Success);
        assert_eq!(ob.local_update_id, 101);
    }

    #[test]
    fn test_apply_event_skipped() {
        let mut ob = OrderBook::new("BTCUSDT".to_string(), MarketType::Spot);
        ob.local_update_id = 100;

        let event = DepthEvent {
            first_update_id: 95,
            final_update_id: 99,
            event_time: 1000,
            bids: vec![],
            asks: vec![],
        };

        let result = ob.apply_event(&event).unwrap();
        assert_eq!(result, ApplyResult::Skipped);
        assert_eq!(ob.local_update_id, 100); // 不变
    }

    #[test]
    fn test_apply_event_gap_detected() {
        let mut ob = OrderBook::new("BTCUSDT".to_string(), MarketType::Spot);
        ob.local_update_id = 100;

        let event = DepthEvent {
            first_update_id: 103,
            final_update_id: 104,
            event_time: 1000,
            bids: vec![],
            asks: vec![],
        };

        let result = ob.apply_event(&event).unwrap();
        assert_eq!(result, ApplyResult::GapDetected);
    }

    #[test]
    fn test_export_top_bids() {
        let mut ob = OrderBook::new("BTCUSDT".to_string(), MarketType::Spot);
        ob.local_update_id = 100;

        // 添加多个买盘价位
        ob.bids.insert(Decimal::from(50000), Decimal::from(1));
        ob.bids.insert(Decimal::from(49999), Decimal::from(2));
        ob.bids.insert(Decimal::from(49998), Decimal::from(3));

        let view = ob.export_top(Side::Bid, 2);
        assert_eq!(view.entries.len(), 2);
        assert_eq!(view.entries[0].price, Decimal::from(50000)); // 最高买价
        assert_eq!(view.entries[1].price, Decimal::from(49999));
    }

    #[test]
    fn test_export_top_asks() {
        let mut ob = OrderBook::new("BTCUSDT".to_string(), MarketType::Spot);
        ob.local_update_id = 100;

        // 添加多个卖盘价位
        ob.asks.insert(Decimal::from(50001), Decimal::from(1));
        ob.asks.insert(Decimal::from(50002), Decimal::from(2));
        ob.asks.insert(Decimal::from(50003), Decimal::from(3));

        let view = ob.export_top(Side::Ask, 2);
        assert_eq!(view.entries.len(), 2);
        assert_eq!(view.entries[0].price, Decimal::from(50001)); // 最低卖价
        assert_eq!(view.entries[1].price, Decimal::from(50002));
    }

    #[test]
    fn test_delete_level_with_zero_qty() {
        let mut ob = OrderBook::new("BTCUSDT".to_string(), MarketType::Spot);
        ob.local_update_id = 100;

        // 先添加一个价位
        ob.bids.insert(Decimal::from(50000), Decimal::from(1));
        assert_eq!(ob.size(), 1);

        // 应用数量为 0 的事件来删除
        let event = DepthEvent {
            first_update_id: 101,
            final_update_id: 101,
            event_time: 1000,
            bids: vec![(Decimal::from(50000), Decimal::ZERO)],
            asks: vec![],
        };

        ob.apply_event(&event).unwrap();
        assert_eq!(ob.size(), 0);
    }
}
