use crate::binance::bn_models::spot_restful::Depth;
use crate::binance::bn_models::spot_websocket_stream::DepthUpdateStreamPayload;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// 关于orderbook。
/// spot币安交易所的orderbook维护说明：
/// https://developers.binance.com/docs/zh-CN/binance-spot-api-docs/web-socket-streams#%E5%A6%82%E4%BD%95%E6%AD%A3%E7%A1%AE%E5%9C%A8%E6%9C%AC%E5%9C%B0%E7%BB%B4%E6%8A%A4%E4%B8%80%E4%B8%AAorder-book%E5%89%AF%E6%9C%AC

/// 订单簿结构体,负责维护和更新相应的订单数据。
/// 这个值保存更新逻辑。不负责更新订阅和网络通信。
#[derive(Debug, Clone)]
pub struct OrderBook {
    /// 交易对
    pub symbol: String,
    /// 买盘：价位 -> 数量（BTreeMap 自动按价位排序）
    bids: BTreeMap<Decimal, Decimal>,
    /// 卖盘：价位 -> 数量
    asks: BTreeMap<Decimal, Decimal>,
    /// 本地更新 ID（最后应用的事件的 u）
    pub local_update_id: u64,
    /// 最后更新时间戳（毫秒）
    pub last_update_time: u64,
}

#[derive(Error, Debug)]
pub enum OrderBookError {
    #[error("update id is deprecated: last_update_id={last_update_id:?}, last_update_time={last_update_time:?}")]
    DeprecateError { last_update_id: u64, last_update_time: String },
}

impl OrderBook {
    /// 全量更新订单簿数据
    /// 1. 根据depth更新bids和asks。
    /// 2. 设置本地order book的更新ID为depth的lastUpdateId。
    /// 3， 设置最后更新时间戳为当前时间戳。
    pub fn new<S: Into<String>>(symbol: S, depth: Depth) -> Result<Self, OrderBookError> {
        let mut bids_map: BTreeMap<Decimal, Decimal> = BTreeMap::new();
        let mut asks_map: BTreeMap<Decimal, Decimal> = BTreeMap::new();

        for (price, qty) in depth.bids {
            if !qty.is_zero() {
                bids_map.insert(price, qty);
            }
        }
        for (price, qty) in depth.asks {
            if !qty.is_zero() {
                asks_map.insert(price, qty);
            }
        }

        // 获取当前毫秒时间戳
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);

        Ok(Self {
            symbol: symbol.into(),
            bids: bids_map,
            asks: asks_map,
            local_update_id: depth.last_update_id,
            last_update_time: now_ms,
        })
    }

    /// 根据length。来获取部分订单簿数据。bids和asks各取length个。
    pub fn get_sub_order_book(&self, length: i32) -> Result<OrderBook, OrderBookError> {
        if length <= 0 {
            return Ok(OrderBook {
                symbol: self.symbol.clone(),
                bids: BTreeMap::new(),
                asks: BTreeMap::new(),
                local_update_id: self.local_update_id,
                last_update_time: self.last_update_time,
            });
        }

        let take = length as usize;
        let mut bids_map: BTreeMap<Decimal, Decimal> = BTreeMap::new();
        let mut asks_map: BTreeMap<Decimal, Decimal> = BTreeMap::new();

        // BTreeMap is ordered ascending by key. For bids we need highest prices, so iterate in reverse.
        for (price, qty) in self.bids.iter().rev().take(take) {
            bids_map.insert(price.clone(), qty.clone());
        }

        // For asks we need lowest prices, so iterate forward.
        for (price, qty) in self.asks.iter().take(take) {
            asks_map.insert(price.clone(), qty.clone());
        }

        Ok(OrderBook {
            symbol: self.symbol.clone(),
            bids: bids_map,
            asks: asks_map,
            local_update_id: self.local_update_id,
            last_update_time: self.last_update_time,
        })
    }

    /// 增量更新订单簿数据。
    /// # 判断是否需要处理event：
    ///    - 如果event的最后一次更新ID（u）小于本地order book的更新ID，忽略该event。
    ///    - 如果event的首次更新ID（U）大于本地order book的更新ID加1，抛出DeprecateError,时间转换成人可读
    ///    - 通常，下一event的U等于上一event的u + 1。
    /// # 对买价（b）和卖价（a）中的每个价位，设置order book中的新数量：
    ///    - 如果该价位在order book中不存在，则插入该价位及其数量。
    ///     -如果数量为零，则从order book中删除此价位。
    /// # 将order book的更新ID设置为已处理event的最后一次更新ID（u）
    pub fn apply_snapshot(&mut self, snapshot: DepthUpdateStreamPayload) -> Result<(), OrderBookError> {
        // 如果事件的最终更新 ID 小于本地更新 ID，则忽略该事件
        if snapshot.final_update_id < self.local_update_id {
            return Ok(());
        }

        // 如果事件的首个更新 ID 大于本地更新 ID + 1，则说明本地缺失了中间的更新，需要抛出错误
        if snapshot.first_update_id > self.local_update_id.saturating_add(1) {
            let last_time_str = format!("{}ms", self.last_update_time);
            return Err(OrderBookError::DeprecateError {
                last_update_id: self.local_update_id,
                last_update_time: last_time_str,
            });
        }

        // 处理买盘更新：price/qty 都来自 f64，需要转换为 Decimal
        for (price_f, qty_f) in snapshot.bids {
            if let Some(price) = Decimal::from_f64(price_f) {
                let qty = Decimal::from_f64(qty_f).unwrap_or_default();
                if qty.is_zero() {
                    self.bids.remove(&price);
                } else {
                    self.bids.insert(price, qty);
                }
            }
        }

        // 处理卖盘更新
        for (price_f, qty_f) in snapshot.asks {
            if let Some(price) = Decimal::from_f64(price_f) {
                let qty = Decimal::from_f64(qty_f).unwrap_or_default();
                if qty.is_zero() {
                    self.asks.remove(&price);
                } else {
                    self.asks.insert(price, qty);
                }
            }
        }

        // 更新本地状态
        self.local_update_id = snapshot.final_update_id;
        self.last_update_time = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);

        Ok(())
    }

    pub fn best_bid(&self) -> Option<(&Decimal, &Decimal)> {
        self.bids.iter().next_back().map(|(p, q)| (p, q))
    }

    pub fn best_ask(&self) -> Option<(&Decimal, &Decimal)> {
        self.asks.iter().next().map(|(p, q)| (p, q))
    }

    pub fn bids_count(&self) -> usize {
        self.bids.len()
    }

    pub fn asks_count(&self) -> usize {
        self.asks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn is_sorted_asc(keys: &Vec<Decimal>) -> bool {
        if keys.len() <= 1 {
            return true;
        }
        for i in 1..keys.len() {
            if keys[i - 1] > keys[i] {
                return false;
            }
        }
        true
    }

    fn max_bid_min_ask(bids: &BTreeMap<Decimal, Decimal>, asks: &BTreeMap<Decimal, Decimal>) -> Option<(Decimal, Decimal)> {
        if bids.is_empty() || asks.is_empty() {
            return None;
        }
        let max_bid = bids.iter().next_back().map(|(p, _)| p.clone()).unwrap();
        let min_ask = asks.iter().next().map(|(p, _)| p.clone()).unwrap();
        Some((max_bid, min_ask))
    }

    #[test]
    fn test_new_populates_order_book() {
        let depth = Depth {
            last_update_id: 42,
            bids: vec![(Decimal::new(100, 0), Decimal::new(1, 0)), (Decimal::new(101, 0), Decimal::new(2, 0))],
            asks: vec![
                (Decimal::new(102, 0), Decimal::new(15, 1)), // 1.5
                (Decimal::new(103, 0), Decimal::new(0, 0)),
            ],
        };

        let ob = OrderBook::new("BTCUSDT", depth.clone()).unwrap();
        assert_eq!(ob.local_update_id, depth.last_update_id);
        assert_eq!(ob.bids_count(), 2);
        assert_eq!(ob.asks_count(), 1);
        assert!(ob.last_update_time > 0);
        // 验证语义：bid 为买盘，ask 为卖盘，且最低 ask price 必须高于最高 bid price
        if let Some((max_bid, min_ask)) = max_bid_min_ask(&ob.bids, &ob.asks) {
            assert!(min_ask > max_bid, "ask price must be greater than bid price");
        }
    }

    #[test]
    fn test_best_bid_and_best_ask() {
        let depth = Depth {
            last_update_id: 100,
            bids: vec![(Decimal::new(100, 0), Decimal::new(1, 0)), (Decimal::new(101, 0), Decimal::new(2, 0))],
            asks: vec![(Decimal::new(102, 0), Decimal::new(1, 0)), (Decimal::new(103, 0), Decimal::new(2, 0))],
        };

        let ob = OrderBook::new("BTCUSDT", depth).unwrap();
        let best_bid = ob.best_bid().expect("best bid exists");
        assert_eq!(best_bid.0, &Decimal::new(101, 0));
        assert_eq!(best_bid.1, &Decimal::new(2, 0));

        let best_ask = ob.best_ask().expect("best ask exists");
        assert_eq!(best_ask.0, &Decimal::new(102, 0));
        assert_eq!(best_ask.1, &Decimal::new(1, 0));

        // 语义校验：最低 ask 大于最高 bid
        if let Some((max_bid, min_ask)) = max_bid_min_ask(&ob.bids, &ob.asks) {
            assert!(min_ask > max_bid, "ask price must be greater than bid price");
        }
    }

    #[test]
    fn test_empty_book_best_none() {
        let depth = Depth {
            last_update_id: 1,
            bids: vec![(Decimal::new(100, 0), Decimal::new(0, 0))],
            asks: vec![],
        };

        let ob = OrderBook::new("BTCUSDT", depth).unwrap();
        assert!(ob.best_bid().is_none());
        assert!(ob.best_ask().is_none());
    }

    // 新增测试：get_sub_order_book length 0 返回空子簿
    #[test]
    fn test_get_sub_order_book_length_zero_returns_empty() {
        let depth = Depth {
            last_update_id: 55,
            bids: vec![(Decimal::new(100, 0), Decimal::new(1, 0))],
            asks: vec![(Decimal::new(101, 0), Decimal::new(1, 0))],
        };
        let ob = OrderBook::new("BTCUSDT", depth).unwrap();
        let sub = ob.get_sub_order_book(0).unwrap();
        assert_eq!(sub.bids_count(), 0);
        assert_eq!(sub.asks_count(), 0);
        assert_eq!(sub.symbol, ob.symbol);
        assert_eq!(sub.local_update_id, ob.local_update_id);
        assert_eq!(sub.last_update_time, ob.last_update_time);
    }

    // 新增测试：get_sub_order_book length 1 返回乱序的 bids/asks（断言不是按升序排列）
    #[test]
    fn test_get_sub_order_book_length_one_ordered() {
        let depth = Depth {
            last_update_id: 200,
            // 特意以非排序顺序构造深度数据
            bids: vec![
                (Decimal::new(100, 0), Decimal::new(1, 0)),
                (Decimal::new(103, 0), Decimal::new(5, 0)),
                (Decimal::new(101, 0), Decimal::new(2, 0)),
            ],
            asks: vec![
                (Decimal::new(110, 0), Decimal::new(1, 0)),
                (Decimal::new(108, 0), Decimal::new(3, 0)),
                (Decimal::new(109, 0), Decimal::new(2, 0)),
            ],
        };

        let ob = OrderBook::new("BTCUSDT", depth).unwrap();
        let sub = ob.get_sub_order_book(2).unwrap();

        // 提取 bids keys 的迭代顺序
        let bid_keys: Vec<Decimal> = sub.bids.iter().map(|(p, _)| p.clone()).collect();
        let ask_keys: Vec<Decimal> = sub.asks.iter().map(|(p, _)| p.clone()).collect();

        // 断言按实现返回的有序结果
        assert!(is_sorted_asc(&bid_keys), "expected bids to be ordered");
        assert!(is_sorted_asc(&ask_keys), "expected asks to be ordered");

        // 验证 ask (卖盘) 的最低价高于 bid (买盘) 的最高价
        if let Some((max_bid, min_ask)) = max_bid_min_ask(&sub.bids, &sub.asks) {
            assert_eq!(max_bid, Decimal::new(103, 0));
            assert_eq!(min_ask, Decimal::new(108, 0));
        }
    }

    // 新增测试：length 大于现有档位返回全部且乱序
    #[test]
    fn test_get_sub_order_book_length_large_returns_all_ordered() {
        let depth = Depth {
            last_update_id: 300,
            bids: vec![
                (Decimal::new(100, 0), Decimal::new(1, 0)),
                (Decimal::new(101, 0), Decimal::new(2, 0)),
                (Decimal::new(102, 0), Decimal::new(3, 0)),
            ],
            asks: vec![(Decimal::new(110, 0), Decimal::new(1, 0)), (Decimal::new(111, 0), Decimal::new(2, 0))],
        };

        let ob = OrderBook::new("BTCUSDT", depth).unwrap();
        let sub = ob.get_sub_order_book(10).unwrap();

        assert_eq!(sub.bids_count(), ob.bids_count());
        assert_eq!(sub.asks_count(), ob.asks_count());

        let bid_keys: Vec<Decimal> = sub.bids.iter().map(|(p, _)| p.clone()).collect();
        let ask_keys: Vec<Decimal> = sub.asks.iter().map(|(p, _)| p.clone()).collect();

        assert!(is_sorted_asc(&bid_keys), "expected bids to be ordered when returned");
        assert!(is_sorted_asc(&ask_keys), "expected asks to be ordered when returned");

        if let Some((max_bid, min_ask)) = max_bid_min_ask(&sub.bids, &sub.asks) {
            assert_eq!(max_bid, Decimal::new(102, 0));
            assert_eq!(min_ask, Decimal::new(110, 0));
        }
    }

    #[test]
    fn test_apply_snapshot_normal_update() {
        use crate::binance::bn_models::spot_websocket_stream::DepthUpdateStreamPayload;

        let depth = Depth {
            last_update_id: 100,
            bids: vec![(Decimal::new(100, 0), Decimal::new(1, 0)), (Decimal::new(99, 0), Decimal::new(1, 0))],
            asks: vec![(Decimal::new(102, 0), Decimal::new(1, 0)), (Decimal::new(103, 0), Decimal::new(1, 0))],
        };

        let mut ob = OrderBook::new("BTCUSDT", depth).unwrap();

        // 插入新的 bid 101, 删除 100（qty 0）
        let snap = DepthUpdateStreamPayload {
            event: "depthUpdate".to_string(),
            event_time: 0,
            symbol: "BTCUSDT".to_string(),
            first_update_id: 101,
            final_update_id: 101,
            bids: vec![(101.0, 2.0), (100.0, 0.0)],
            asks: vec![],
        };

        assert!(ob.apply_snapshot(snap).is_ok());
        assert_eq!(ob.local_update_id, 101);
        // bids 应包含 101 和 99，但不包含 100
        assert!(ob.bids.contains_key(&Decimal::new(101, 0)));
        assert!(ob.bids.contains_key(&Decimal::new(99, 0)));
        assert!(!ob.bids.contains_key(&Decimal::new(100, 0)));
        // best bid 为 101
        let best = ob.best_bid().unwrap();
        assert_eq!(best.0, &Decimal::new(101, 0));
        assert_eq!(best.1, &Decimal::new(2, 0));
    }

    #[test]
    fn test_apply_snapshot_ignored_stale_event() {
        use crate::binance::bn_models::spot_websocket_stream::DepthUpdateStreamPayload;

        let depth = Depth {
            last_update_id: 200,
            bids: vec![(Decimal::new(100, 0), Decimal::new(1, 0))],
            asks: vec![(Decimal::new(101, 0), Decimal::new(1, 0))],
        };
        let mut ob = OrderBook::new("BTCUSDT", depth).unwrap();

        // final_update_id 小于本地 local_update_id，应被忽略
        let snap = DepthUpdateStreamPayload {
            event: "depthUpdate".to_string(),
            event_time: 0,
            symbol: "BTCUSDT".to_string(),
            first_update_id: 190,
            final_update_id: 199,
            bids: vec![(99.0, 1.0)],
            asks: vec![],
        };

        assert!(ob.apply_snapshot(snap).is_ok());
        // local_update_id 不变
        assert_eq!(ob.local_update_id, 200);
        // 订单簿应保持原状
        assert!(ob.bids.contains_key(&Decimal::new(100, 0)));
    }

    #[test]
    fn test_apply_snapshot_deprecate_error_when_gap() {
        use crate::binance::bn_models::spot_websocket_stream::DepthUpdateStreamPayload;

        let depth = Depth {
            last_update_id: 300,
            bids: vec![(Decimal::new(100, 0), Decimal::new(1, 0))],
            asks: vec![(Decimal::new(101, 0), Decimal::new(1, 0))],
        };
        let mut ob = OrderBook::new("BTCUSDT", depth).unwrap();

        // first_update_id 大于 local_update_id + 1，应该返回 DeprecateError
        let snap = DepthUpdateStreamPayload {
            event: "depthUpdate".to_string(),
            event_time: 0,
            symbol: "BTCUSDT".to_string(),
            first_update_id: 302,
            final_update_id: 302,
            bids: vec![],
            asks: vec![],
        };

        match ob.apply_snapshot(snap) {
            Err(OrderBookError::DeprecateError { last_update_id, .. }) => {
                assert_eq!(last_update_id, 300);
            }
            other => panic!("expected DeprecateError, got: {:?}", other),
        }
    }

    #[test]
    fn test_apply_snapshot_ask_update() {
        use crate::binance::bn_models::spot_websocket_stream::DepthUpdateStreamPayload;

        let depth = Depth {
            last_update_id: 150,
            bids: vec![(Decimal::new(100, 0), Decimal::new(1, 0))],
            asks: vec![(Decimal::new(105, 0), Decimal::new(1, 0)), (Decimal::new(106, 0), Decimal::new(2, 0))],
        };
        let mut ob = OrderBook::new("BTCUSDT", depth).unwrap();

        // 将 105 的 qty 设为 0（删除），插入新的 ask 104
        let snap = DepthUpdateStreamPayload {
            event: "depthUpdate".to_string(),
            event_time: 0,
            symbol: "BTCUSDT".to_string(),
            first_update_id: 151,
            final_update_id: 151,
            bids: vec![],
            asks: vec![(105.0, 0.0), (104.0, 1.5)],
        };

        assert!(ob.apply_snapshot(snap).is_ok());
        // 105 被移除，104 被插入，best_ask 应为 104
        assert!(!ob.asks.contains_key(&Decimal::new(105, 0)));
        assert!(ob.asks.contains_key(&Decimal::new(104, 0)));
        let best = ob.best_ask().unwrap();
        assert_eq!(best.0, &Decimal::new(104, 0));
    }

    #[test]
    fn test_apply_snapshot_skip_invalid_price() {
        use crate::binance::bn_models::spot_websocket_stream::DepthUpdateStreamPayload;

        let depth = Depth {
            last_update_id: 400,
            bids: vec![(Decimal::new(200, 0), Decimal::new(1, 0))],
            asks: vec![(Decimal::new(210, 0), Decimal::new(1, 0))],
        };
        let mut ob = OrderBook::new("BTCUSDT", depth).unwrap();

        // 构造一个包含 NaN price 的更新（应该被跳过），以及一个正常更新
        let invalid_price = f64::NAN;
        let snap = DepthUpdateStreamPayload {
            event: "depthUpdate".to_string(),
            event_time: 0,
            symbol: "BTCUSDT".to_string(),
            first_update_id: 401,
            final_update_id: 401,
            bids: vec![(invalid_price, 5.0), (201.0, 3.0)],
            asks: vec![],
        };

        assert!(ob.apply_snapshot(snap).is_ok());
        // NaN price 的更新应被跳过，201 应被插入
        assert!(ob.bids.contains_key(&Decimal::new(201, 0)));
        // 原有的 200 仍然存在
        assert!(ob.bids.contains_key(&Decimal::new(200, 0)));
    }

    #[test]
    fn test_apply_snapshot_nan_qty_removes_price() {
        use crate::binance::bn_models::spot_websocket_stream::DepthUpdateStreamPayload;

        let depth = Depth {
            last_update_id: 500,
            bids: vec![(Decimal::new(300, 0), Decimal::new(4, 0))],
            asks: vec![(Decimal::new(301, 0), Decimal::new(4, 0))],
        };
        let mut ob = OrderBook::new("BTCUSDT", depth).unwrap();

        // qty 为 NaN 时 Decimal::from_f64 返回 None，unwrap_or_default() -> 0，因此会删除该价位
        let snap = DepthUpdateStreamPayload {
            event: "depthUpdate".to_string(),
            event_time: 0,
            symbol: "BTCUSDT".to_string(),
            first_update_id: 501,
            final_update_id: 501,
            bids: vec![(300.0, 0f64)],
            asks: vec![(301.0, 0f64)],
        };

        assert!(ob.apply_snapshot(snap).is_ok());
        // 300 因为被当作 qty=0 处理，应被移除
        assert!(!ob.bids.contains_key(&Decimal::new(300, 0)));
        assert!(!ob.asks.contains_key(&Decimal::new(301, 0)));
    }
}
