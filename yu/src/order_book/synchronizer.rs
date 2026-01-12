use crate::errors::YuError;
use crate::order_book::order_book::{ApplyResult, DepthEvent, OrderBook, Snapshot};
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

/// 同步状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    /// 未初始化状态
    Uninitialized,
    /// 快照同步中
    Snapshotting,
    /// 已同步状态
    Synced,
}

impl std::fmt::Display for SyncState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncState::Uninitialized => write!(f, "Uninitialized"),
            SyncState::Snapshotting => write!(f, "Snapshotting"),
            SyncState::Synced => write!(f, "Synced"),
        }
    }
}

/// 同步统计信息
#[derive(Debug, Clone)]
pub struct SyncStats {
    /// 重连次数
    pub reconnect_count: u64,
    /// 重同步次数
    pub resync_count: u64,
    /// 当前缓存大小
    pub cache_size: usize,
    /// 最后处理时间（UNIX 时间戳，毫秒）
    pub last_process_time: u64,
    /// 平均处理延迟（毫秒）
    pub avg_process_delay_ms: f64,
    /// 处理过的事件总数
    pub processed_events: u64,
}

impl Default for SyncStats {
    fn default() -> Self {
        Self {
            reconnect_count: 0,
            resync_count: 0,
            cache_size: 0,
            last_process_time: 0,
            avg_process_delay_ms: 0.0,
            processed_events: 0,
        }
    }
}

/// 事件缓存与同步管理器
#[derive(Debug)]
pub struct Synchronizer {
    /// 订单簿
    pub order_book: OrderBook,
    /// 事件缓存队列
    event_cache: VecDeque<DepthEvent>,
    /// 最大缓存大小
    max_cache_size: usize,
    /// 当前同步状态
    state: SyncState,
    /// 统计信息
    stats: SyncStats,
    /// 处理延迟累计器（用于计算平均值）
    total_delay_ms: f64,
}

impl Synchronizer {
    /// 创建新的同步管理器
    pub fn new(symbol: String, market_type: crate::order_book::order_book::MarketType, max_cache_size: usize) -> Self {
        Self {
            order_book: OrderBook::new(symbol, market_type),
            event_cache: VecDeque::with_capacity(max_cache_size),
            max_cache_size,
            state: SyncState::Uninitialized,
            stats: SyncStats::default(),
            total_delay_ms: 0.0,
        }
    }

    /// 获取当前状态
    pub fn state(&self) -> SyncState {
        self.state
    }

    /// 初始化：应用快照并回放缓存中的有效事件
    pub fn init(&mut self, snapshot: &Snapshot) -> Result<(), YuError> {
        // 丢弃缓存中 u <= snapshot.last_update_id 的事件
        self.event_cache.retain(|ev| ev.final_update_id > snapshot.last_update_id);

        // 应用快照
        self.order_book.apply_snapshot(snapshot)?;

        // 回放剩余事件
        let remaining_events: Vec<_> = self.event_cache.iter().cloned().collect();
        for event in remaining_events {
            match self.order_book.apply_event(&event) {
                Ok(ApplyResult::Success) => {
                    self.stats.processed_events += 1;
                }
                Ok(ApplyResult::GapDetected) => {
                    // 回放过程中检测到缺口，清空缓存并转移状态
                    self.event_cache.clear();
                    self.state = SyncState::Uninitialized;
                    return Err(YuError::new("Gap detected during event replay after snapshot"));
                }
                _ => {}
            }
        }

        // 转移状态到 Synced
        self.state = SyncState::Synced;
        self.stats.cache_size = self.event_cache.len();

        Ok(())
    }

    /// 处理单个事件，根据结果判断是否需要重同步
    pub fn on_event(&mut self, ev: DepthEvent) -> Result<bool, YuError> {
        // 缓存事件
        if self.event_cache.len() < self.max_cache_size {
            self.event_cache.push_back(ev.clone());
        } else {
            // 缓存满，需要重同步
            self.event_cache.clear();
            self.state = SyncState::Uninitialized;
            return Err(YuError::new("Event cache overflow, resync required"));
        }

        self.stats.cache_size = self.event_cache.len();

        // 如果处于未初始化或快照同步状态，不处理
        if self.state != SyncState::Synced {
            return Ok(false);
        }

        // 尝试应用事件
        match self.order_book.apply_event(&ev)? {
            ApplyResult::Success => {
                self.stats.processed_events += 1;
                self.update_last_process_time(ev.event_time);
                Ok(false)
            }
            ApplyResult::Skipped => {
                // 过期事件，忽略
                Ok(false)
            }
            ApplyResult::GapDetected => {
                // 检测到缺口，需要重同步
                Ok(true)
            }
        }
    }

    /// 触发重同步
    pub fn trigger_resync(&mut self) -> Result<(), YuError> {
        self.event_cache.clear();
        self.order_book.clear();
        self.state = SyncState::Uninitialized;
        self.stats.resync_count += 1;
        self.stats.cache_size = 0;
        Ok(())
    }

    /// 获取统计信息
    pub fn get_stats(&self) -> SyncStats {
        self.stats.clone()
    }

    /// 更新最后处理时间和延迟
    fn update_last_process_time(&mut self, event_time: u64) {
        let now = current_time_millis();
        if now >= event_time {
            let delay = now - event_time;
            self.total_delay_ms += delay as f64;
            let avg = self.total_delay_ms / self.stats.processed_events as f64;
            self.stats.avg_process_delay_ms = avg;
        }
        self.stats.last_process_time = now;
    }

    /// 记录一次重新连接
    pub fn record_reconnect(&mut self) {
        self.stats.reconnect_count += 1;
    }

    /// 获取缓存中的事件数量
    pub fn cache_len(&self) -> usize {
        self.event_cache.len()
    }

    /// 获取本地更新 ID
    pub fn local_update_id(&self) -> u64 {
        self.order_book.local_update_id
    }
}

/// 获取当前时间（毫秒）
fn current_time_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order_book::order_book::MarketType;

    #[test]
    fn test_synchronizer_create() {
        let sync = Synchronizer::new("BTCUSDT".to_string(), MarketType::Spot, 1000);
        assert_eq!(sync.state(), SyncState::Uninitialized);
        assert_eq!(sync.cache_len(), 0);
    }

    #[test]
    fn test_synchronizer_init() {
        let mut sync = Synchronizer::new("BTCUSDT".to_string(), MarketType::Spot, 1000);

        let snapshot = Snapshot {
            last_update_id: 100,
            bids: vec![(Decimal::from(50000), Decimal::from(1))],
            asks: vec![(Decimal::from(50001), Decimal::from(1))],
        };

        sync.init(&snapshot).unwrap();
        assert_eq!(sync.state(), SyncState::Synced);
        assert_eq!(sync.local_update_id(), 100);
    }

    #[test]
    fn test_synchronizer_on_event() {
        let mut sync = Synchronizer::new("BTCUSDT".to_string(), MarketType::Spot, 1000);

        let snapshot = Snapshot {
            last_update_id: 100,
            bids: vec![],
            asks: vec![],
        };
        sync.init(&snapshot).unwrap();

        let event = DepthEvent {
            first_update_id: 101,
            final_update_id: 101,
            event_time: 1000,
            bids: vec![(Decimal::from(50000), Decimal::from(1))],
            asks: vec![],
        };

        let needs_resync = sync.on_event(event).unwrap();
        assert!(!needs_resync);
        assert_eq!(sync.cache_len(), 1);
    }

    #[test]
    fn test_synchronizer_cache_overflow() {
        let mut sync = Synchronizer::new("BTCUSDT".to_string(), MarketType::Spot, 1);

        let snapshot = Snapshot {
            last_update_id: 100,
            bids: vec![],
            asks: vec![],
        };
        sync.init(&snapshot).unwrap();

        let event1 = DepthEvent {
            first_update_id: 101,
            final_update_id: 101,
            event_time: 1000,
            bids: vec![],
            asks: vec![],
        };
        let event2 = DepthEvent {
            first_update_id: 102,
            final_update_id: 102,
            event_time: 1001,
            bids: vec![],
            asks: vec![],
        };

        sync.on_event(event1).unwrap();
        let result = sync.on_event(event2);
        assert!(result.is_err());
        assert_eq!(sync.state(), SyncState::Uninitialized);
    }

    #[test]
    fn test_synchronizer_replay_events() {
        let mut sync = Synchronizer::new("BTCUSDT".to_string(), MarketType::Spot, 1000);

        // 先加入一些事件到缓存
        let event1 = DepthEvent {
            first_update_id: 101,
            final_update_id: 101,
            event_time: 1000,
            bids: vec![(Decimal::from(50000), Decimal::from(1))],
            asks: vec![],
        };

        let event2 = DepthEvent {
            first_update_id: 102,
            final_update_id: 102,
            event_time: 1001,
            bids: vec![(Decimal::from(50000), Decimal::from(2))],
            asks: vec![],
        };

        sync.event_cache.push_back(event1);
        sync.event_cache.push_back(event2);

        // 现在初始化快照
        let snapshot = Snapshot {
            last_update_id: 100,
            bids: vec![],
            asks: vec![],
        };

        sync.init(&snapshot).unwrap();

        // 应该已回放事件
        assert_eq!(sync.local_update_id(), 102);
    }

    #[test]
    fn test_synchronizer_trigger_resync() {
        let mut sync = Synchronizer::new("BTCUSDT".to_string(), MarketType::Spot, 1000);

        let snapshot = Snapshot {
            last_update_id: 100,
            bids: vec![],
            asks: vec![],
        };
        sync.init(&snapshot).unwrap();

        sync.trigger_resync().unwrap();
        assert_eq!(sync.state(), SyncState::Uninitialized);
        assert_eq!(sync.cache_len(), 0);
        assert_eq!(sync.stats.resync_count, 1);
    }

    // 需要导入 Decimal
    use rust_decimal::Decimal;
}
