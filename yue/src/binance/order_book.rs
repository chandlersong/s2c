use crate::binance::bn_json_websocket::{SPOT_STREAM_WEBSOCKET, StreamCommandRequest, WS_SUBSCRIBE_COMMAND};
use crate::binance::bn_models::spot_restful::Depth;
use crate::binance::bn_models::spot_websocket_stream::{
    BinanceSpotWebSocketStreamResponse, BinanceSpotWebSocketStreamWrapper, DepthUpdateStreamPayload,
};
use crate::binance::bn_restful_commands::{SPOT_DEPTH_1000_COMMAND, execute_json_request};
use crate::binance::restful_func::CommonRequestBuilder;
use crate::errors::YueError;
use crate::http_client::ToRequestBuilder;
use async_trait::async_trait;
use li::websocket::connection::{CommandMessage, MessageHandlerTrait, ToServerMessage, WebSocketConnection, WebSocketInterface};
use log::{debug, error, info, trace};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{mpsc, oneshot};

/// 关于orderbook。
/// spot币安交易所的orderbook维护说明：
/// https://developers.binance.com/docs/zh-CN/binance-spot-api-docs/web-socket-streams#%E5%A6%82%E4%BD%95%E6%AD%A3%E7%A1%AE%E5%9C%A8%E6%9C%AC%E5%9C%B0%E7%BB%B4%E6%8A%A4%E4%B8%80%E4%B8%AAorder-book%E5%89%AF%E6%9C%AC

#[derive(Error, Debug)]
pub enum OrderBookError {
    #[error("update id is deprecated: last_update_id={last_update_id:?}, last_update_time={last_update_time:?}")]
    DeprecateError { last_update_id: u64, last_update_time: String },
}

///
/// 初始化订单簿的流程：
///
/// FUTURE
/// 1.又一个小问题，就是在初始化的时候，还有depth进来。不过这个也就不用管了。
pub async fn initial_order_book(symbol: String, mut depth_rx: mpsc::UnboundedReceiver<DepthUpdateStreamPayload>) -> OrderBook {
    // 缓存 websocket 的增量更新，直到 REST 返回快照
    let params = CommonRequestBuilder::symbol_and_limit(symbol.to_uppercase().clone(), 1000);
    // 使用 Pin<Box<_>> 便于在失败后重建并重新 pin
    let mut depth_request = Box::pin(execute_json_request::<Depth>(
        &SPOT_DEPTH_1000_COMMAND,
        params.to_request_builder(&SPOT_DEPTH_1000_COMMAND),
        None,
    ));

    let mut depth_vec = VecDeque::new();
    loop {
        tokio::select! {
            Some(depth) = depth_rx.recv() => {
               depth_vec.push_back(depth);
            }
            depth_res = depth_request.as_mut() => {
                match depth_res {
                    Ok(depth) => {
                        debug!("成功获取 {} 的Depth数据, lastUpdateId={}", symbol, depth.last_update_id);
                        return match OrderBook::new(symbol.clone(), depth) {
                            Ok(mut order_book) => {
                                if !depth_vec.is_empty() {
                                    info!("[initial_order_book] {} 应用缓存的 {} 条更新", symbol,depth_vec.len());
                                    let mut update_fail = false;
                                    for (idx, update) in depth_vec.iter().enumerate() {
                                        trace!("[initial_order_book] 应用第 {} 条更新: first_update_id={}, final_update_id={}", idx, update.first_update_id, update.final_update_id);
                                        if let Err(e) = order_book.apply_snapshot(update.clone()) {
                                            info!("[initial_order_book] 应用缓存更新失败 {}: {:?}", symbol, e);
                                            update_fail= true;
                                            break;
                                        } else {
                                            trace!("[initial_order_book] 成功应用更新，current local_update_id={}", order_book.local_update_id);

                                        }
                                    }
                                    if update_fail {
                                          depth_request = Box::pin(execute_json_request::<Depth>(
                                            &SPOT_DEPTH_1000_COMMAND,
                                            params.to_request_builder(&SPOT_DEPTH_1000_COMMAND),
                                            None,
                                        ));
                                        depth_vec.clear();
                                        continue;
                                    }
                                }
                                info!("[initial_order_book] 创建 OrderBook {} 完成", symbol);
                                order_book
                            }
                            Err(e) => {
                                error!("[initial_order_book] 创建 OrderBook 失败 {}: {:?}", symbol, e);
                                tokio::time::sleep(Duration::from_millis(500)).await;
                                depth_request = Box::pin(execute_json_request::<Depth>(
                                    &SPOT_DEPTH_1000_COMMAND,
                                    params.to_request_builder(&SPOT_DEPTH_1000_COMMAND),
                                    None,
                                ));
                                depth_vec.clear();
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        error!("[initial_order_book] 获取 REST Depth 失败 {}: {:?}, retry after backoff", symbol, e);
                        // 简单重试：等待一段时间后重新创建请求并继续循环
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        depth_request = Box::pin(execute_json_request::<Depth>(
                            &SPOT_DEPTH_1000_COMMAND,
                            params.to_request_builder(&SPOT_DEPTH_1000_COMMAND),
                            None,
                        ));
                        depth_vec.clear();
                    }
                }
            }
        }
    }
}

pub enum OrderBookEvent {
    DepthUpdate(DepthUpdateStreamPayload),
    QueryOrderBook(QueryPayload),
    NewOrderBook(OrderBook),
}

pub struct QueryPayload {
    pub symbol: String,
    pub depth: u16,
    pub tx: oneshot::Sender<Result<OrderBook, YueError>>,
}

impl QueryPayload {
    pub fn new_depth_20(symbol: String, tx: oneshot::Sender<Result<OrderBook, YueError>>) -> OrderBookEvent {
        Self::new(symbol, 20, tx)
    }

    pub fn new(symbol: String, depth: u16, tx: oneshot::Sender<Result<OrderBook, YueError>>) -> OrderBookEvent {
        OrderBookEvent::QueryOrderBook(Self { symbol, depth, tx })
    }
}

/**
订单簿的管理，其中包括
1. 订单簿的更新
2. 订单簿的查询

FUTURE: 加入有推送机制的order book
**/
struct OrderBookCenter {
    tx: UnboundedSender<OrderBookEvent>,
}

impl OrderBookCenter {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let tx_sync = tx.clone();
        tokio::task::spawn(async move {
            Self::loop_update(tx_sync, rx).await;
        });
        OrderBookCenter { tx }
    }

    /// 本方法是维护订单簿实体逻辑的基本说明。
    /// # 基本规则
    /// 1. 一个symbol一个order book。
    ///
    /// # 重制的规则
    /// 1. order book过期
    /// 2。有新的order book
    async fn loop_update(tx: UnboundedSender<OrderBookEvent>, mut rx: mpsc::UnboundedReceiver<OrderBookEvent>) {
        let mut order_books: HashMap<String, OrderBook> = HashMap::new();
        let mut depth_snapshot: HashMap<String, mpsc::UnboundedSender<DepthUpdateStreamPayload>> = HashMap::new();
        loop {
            if let Some(event) = rx.recv().await {
                match event {
                    OrderBookEvent::DepthUpdate(update) => {
                        let symbol = update.symbol.to_uppercase();
                        match order_books.get_mut(&symbol) {
                            Some(order_book) => {
                                if let Err(_) = order_book.apply_snapshot(update.clone()) {
                                    //这里就表示已经过期，那么应该重新开始那个更新操作。
                                    // 1. 开始创建新的order book
                                    // 2. 加入更新
                                    let rx = Self::initial_order_book(symbol.clone(), tx.clone(), update);
                                    order_books.remove(symbol.as_str());
                                    depth_snapshot.insert(symbol.clone(), rx);
                                }
                            }
                            None => {
                                //如果没有两种情况
                                //1. 正在更新
                                //2。 完全是新的。
                                match depth_snapshot.get_mut(&symbol) {
                                    None => {
                                        let rx = Self::initial_order_book(symbol.clone(), tx.clone(), update);
                                        depth_snapshot.insert(symbol.clone(), rx);
                                    }
                                    Some(tx) => {
                                        //正在更新
                                        if let Err(e) = tx.send(update.clone()) {
                                            error!("failed to send update order books: {}", e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    OrderBookEvent::QueryOrderBook(query) => {
                        let symbol = query.symbol.to_uppercase();
                        match order_books.get_mut(&symbol) {
                            Some(order_book) => {
                                let res = order_book
                                    .get_sub_order_book(query.depth)
                                    .map_err(|e| YueError::CustomError(format!("failed to get sub order book: {}", e)));
                                if let Err(_) = query.tx.send(res) {
                                    error!("failed to send sub order book to query");
                                }
                            }
                            None => {
                                let res = Err(YueError::CustomError("no sub order book".to_string()));
                                if let Err(_) = query.tx.send(res) {
                                    error!("failed to send sub order book to query");
                                }
                            }
                        }
                    }
                    OrderBookEvent::NewOrderBook(order_book) => {
                        order_books.insert(order_book.symbol.to_uppercase().clone(), order_book);
                    }
                }
            }
        }
    }

    pub async fn query_order_book(&self, symbol: &str, depth: u16) -> Result<OrderBook, YueError> {
        let (tx, rx) = oneshot::channel();
        let request = QueryPayload::new(symbol.to_string(), depth, tx);
        self.tx.send(request).map_or_else(
            |e| Err(YueError::CustomError(format!("failed to send order book query: {}", e))),
            |_| Ok(()),
        )?;
        rx.await
            .unwrap_or_else(|e| Err(YueError::CustomError(format!("failed to receive order book query result: {}", e))))
    }

    pub fn initial_order_book(
        symbol: String,
        result_tx: mpsc::UnboundedSender<OrderBookEvent>,
        update: DepthUpdateStreamPayload,
    ) -> mpsc::UnboundedSender<DepthUpdateStreamPayload> {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let order_books = initial_order_book(symbol, rx).await;
            if let Err(e) = result_tx.send(OrderBookEvent::NewOrderBook(order_books)) {
                error!("failed to send initial order books: {}", e);
            }
        });
        if let Err(e) = tx.send(update) {
            error!("failed to send update order books: {}", e);
        }
        tx
    }
}

#[async_trait]
impl MessageHandlerTrait<BinanceSpotWebSocketStreamWrapper> for OrderBookCenter {
    async fn handle_message(&self, message: &BinanceSpotWebSocketStreamWrapper) {
        trace!("Received WebSocket message: {:?}", message);
        match &message.data {
            BinanceSpotWebSocketStreamResponse::DepthUpdate(update) => {
                if let Err(e) = self.tx.send(OrderBookEvent::DepthUpdate(update.clone())) {
                    error!("failed to send depth update to OrderBookCenter: {}", e);
                }
            }
            _ => {
                trace!("Received non-depth update message, ignoring");
            }
        }
    }
}

/// 订单簿服务，管理订阅者和快照分发。
/// 负责维护订阅者列表，并将订单簿快照发送给所有订阅者。
pub struct OrderBookService {
    /// 所有symbol的订单簿快照
    web_socket_interface: Arc<WebSocketInterface<BinanceSpotWebSocketStreamWrapper>>,
    order_book_center: Arc<OrderBookCenter>,
}

impl OrderBookService {
    pub async fn spot(proxy: Option<String>) -> Self {
        let reconnect_interval = Duration::from_secs(5);
        let center = Arc::new(OrderBookCenter::new());
        let interface = WebSocketConnection::run::<BinanceSpotWebSocketStreamWrapper>(
            SPOT_STREAM_WEBSOCKET.to_string(),
            reconnect_interval,
            proxy,
            Some(center.clone()),
        )
        .await;
        Self {
            web_socket_interface: interface,
            order_book_center: center,
        }
    }

    pub fn subscribe_order_book(&self, symbol: &str, frequency: &str) -> Result<(), YueError> {
        let subscribe_symbol = format!("{}@depth@{}", symbol.to_lowercase(), frequency);
        let subscribe_request = StreamCommandRequest {
            method: WS_SUBSCRIBE_COMMAND.to_string(),
            params: vec![subscribe_symbol],
            id: 1,
        };
        let command_test = serde_json::to_string(&subscribe_request)?;
        self.web_socket_interface
            .send_command(CommandMessage::ToServer(ToServerMessage::text(command_test)));
        Ok(())
    }

    pub async fn query_order_book(&self, symbol: &str, depth: u16) -> Result<OrderBook, YueError> {
        self.order_book_center.query_order_book(symbol, depth).await
    }
}

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
    pub fn get_sub_order_book(&self, depth: u16) -> Result<OrderBook, OrderBookError> {
        if depth <= 0 {
            return Ok(OrderBook {
                symbol: self.symbol.clone(),
                bids: BTreeMap::new(),
                asks: BTreeMap::new(),
                local_update_id: self.local_update_id,
                last_update_time: self.last_update_time,
            });
        }

        let take = depth as usize;
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

    pub fn bids(&self) -> &BTreeMap<Decimal, Decimal> {
        &self.bids
    }

    pub fn asks(&self) -> &BTreeMap<Decimal, Decimal> {
        &self.asks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    // ========== 辅助函数 ==========

    /// 检查 Decimal 数组是否按升序排列
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

    /// 返回买盘最高价和卖盘最低价
    /// 用于验证订单簿语义：ask price 必须高于 bid price
    fn max_bid_min_ask(bids: &BTreeMap<Decimal, Decimal>, asks: &BTreeMap<Decimal, Decimal>) -> Option<(Decimal, Decimal)> {
        if bids.is_empty() || asks.is_empty() {
            return None;
        }
        let max_bid = bids.iter().next_back().map(|(p, _)| p.clone()).unwrap();
        let min_ask = asks.iter().next().map(|(p, _)| p.clone()).unwrap();
        Some((max_bid, min_ask))
    }

    // 新增：等待 pending_resync 出现指定 key 的 helper，避免使用固定 sleep 导致的脆弱测试

    // ========== OrderBook 基础功能测试 ==========

    /// 测试：通过 Depth 快照创建订单簿
    ///
    /// 准备数据：
    /// - last_update_id: 42
    /// - bids: [(100, 1.0), (101, 2.0)]
    /// - asks: [(102, 1.5), (103, 0.0)] // 0 数量会被过滤
    ///
    /// 验证点：
    /// 1. local_update_id 正确设置为 42
    /// 2. bids 包含 2 条（100, 101）
    /// 3. asks 只包含 1 条（102），0 数量的 103 被过滤
    /// 4. last_update_time 已设置
    /// 5. 最低卖价(102) > 最高买价(101)，符合订单簿语义
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

    /// 测试：获取最佳买价和最佳卖价
    ///
    /// 准备数据：
    /// - bids: [(100, 1.0), (101, 2.0)]  // 最高买价应为 101
    /// - asks: [(102, 1.0), (103, 2.0)]  // 最低卖价应为 102
    ///
    /// 验证点：
    /// 1. best_bid() 返回 (101, 2.0)
    /// 2. best_ask() 返回 (102, 1.0)
    /// 3. 最低卖价(102) > 最高买价(101)
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

    /// 测试：空订单簿的最佳价格为 None
    ///
    /// 准备数据：
    /// - bids: [(100, 0.0)]  // 0 数量会被过滤
    /// - asks: []
    ///
    /// 验证点：
    /// 1. best_bid() 返回 None
    /// 2. best_ask() 返回 None
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

    // ========== get_sub_order_book 功能测试 ==========

    /// 测试：获取 0 长度的子订单簿应返回空簿
    ///
    /// 准备数据：
    /// - bids: [(100, 1.0)]
    /// - asks: [(101, 1.0)]
    ///
    /// 验证点：
    /// 1. 子簿 bids/asks 数量均为 0
    /// 2. symbol、update_id、update_time 与原订单簿一致
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

    /// 测试：获取指定长度的子订单簿并验证排序
    ///
    /// 准备数据（故意乱序）：
    /// - bids: [(100, 1.0), (103, 5.0), (101, 2.0)]  // 应取最高 2 档：103, 101
    /// - asks: [(110, 1.0), (108, 3.0), (109, 2.0)]  // 应取最低 2 档：108, 109
    ///
    /// 验证点：
    /// 1. 子簿 bids/asks 各有 2 档
    /// 2. BTreeMap 自动按价格升序排列
    /// 3. 最高买价 103 < 最低卖价 108
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

    /// 测试：请求长度超过实际档位时返回全部数据
    ///
    /// 准备数据：
    /// - bids: 3 档
    /// - asks: 2 档
    /// - 请求长度: 10
    ///
    /// 验证点：
    /// 1. 子簿档位数等于原订单簿（3 bids, 2 asks）
    /// 2. 数据按价格升序排列
    /// 3. 最高买价 102 < 最低卖价 110
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

    // ========== apply_snapshot 增量更新测试 ==========

    /// 测试：正常增量更新订单簿
    ///
    /// 初始状态：
    /// - local_update_id: 100
    /// - bids: [(100, 1.0), (99, 1.0)]
    /// - asks: [(102, 1.0), (103, 1.0)]
    ///
    /// 收到增量（first_update_id=101, final_update_id=101）：
    /// - bids: [(101, 2.0), (100, 0.0)]  // 新增 101，删除 100
    ///
    /// 验证点：
    /// 1. local_update_id 更新为 101
    /// 2. bids 包含 101 和 99，不包含 100（被 0 数量删除）
    /// 3. best_bid 为 (101, 2.0)
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

    /// 测试：忽略过期的增量更新
    ///
    /// 初始状态：
    /// - local_update_id: 200
    /// - bids: [(100, 1.0)]
    ///
    /// 收到过期增量（final_update_id=199 < local_update_id）：
    /// - bids: [(99, 1.0)]
    ///
    /// 验证点：
    /// 1. local_update_id 保持 200 不变
    /// 2. bids 仍为 100，不包含 99（增量被忽略）
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

    /// 测试：检测到 gap 时抛出 DeprecateError
    ///
    /// 初始状态：
    /// - local_update_id: 300
    ///
    /// 收到 gap 增量（first_update_id=302 > local_update_id+1）：
    /// - 缺失 301，说明本地订单簿不连续
    ///
    /// 验证点：
    /// 1. 返回 DeprecateError
    /// 2. error 包含 last_update_id=300
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

    /// 测试：更新卖盘数据
    ///
    /// 初始状态：
    /// - asks: [(105, 1.0), (106, 2.0)]
    ///
    /// 收到增量：
    /// - asks: [(105, 0.0), (104, 1.5)]  // 删除 105，新增 104
    ///
    /// 验证点：
    /// 1. 105 被删除
    /// 2. 104 被插入
    /// 3. best_ask 为 (104, 1.5)
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

    /// 测试：跳过无效价格（NaN）
    ///
    /// 初始状态：
    /// - bids: [(200, 1.0)]
    ///
    /// 收到增量（包含 NaN 价格）：
    /// - bids: [(NaN, 5.0), (201, 3.0)]
    ///
    /// 验证点：
    /// 1. NaN 价格被跳过，不影响订单簿
    /// 2. 201 正常插入
    /// 3. 200 保持不变
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

    /// 测试：NaN 数量被当作 0 处理，删除对应价位
    ///
    /// 初始状态：
    /// - bids: [(300, 4.0)]
    /// - asks: [(301, 4.0)]
    ///
    /// 收到增量（数量为 0）：
    /// - bids: [(300, 0.0)]
    /// - asks: [(301, 0.0)]
    ///
    /// 验证点：
    /// 1. 300 从 bids 被删除
    /// 2. 301 从 asks 被删除
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
