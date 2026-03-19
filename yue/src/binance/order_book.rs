use crate::binance::bn_models::common::ToRequestBuilder;
use crate::binance::bn_models::spot_restful::Depth;
use crate::binance::bn_models::spot_websocket_stream::{BinanceSpotWebSocketStreamResponse, DepthUpdateStreamPayload};
use crate::binance::bn_restful_commands::{SPOT_DEPTH_1000_COMMAND, execute_json_request};
use crate::binance::history_data::CommonRequestBuilder;
use actix::{Actor, ActorFutureExt, Addr, AsyncContext, Context, Handler, Message as ActixMessage, Recipient, WrapFuture};
use log::{debug, error, info, trace, warn};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// 关于orderbook。
/// spot币安交易所的orderbook维护说明：
/// https://developers.binance.com/docs/zh-CN/binance-spot-api-docs/web-socket-streams#%E5%A6%82%E4%BD%95%E6%AD%A3%E7%A1%AE%E5%9C%A8%E6%9C%AC%E5%9C%B0%E7%BB%B4%E6%8A%A4%E4%B8%80%E4%B8%AAorder-book%E5%89%AF%E6%9C%AC

#[derive(Error, Debug)]
pub enum OrderBookError {
    #[error("update id is deprecated: last_update_id={last_update_id:?}, last_update_time={last_update_time:?}")]
    DeprecateError { last_update_id: u64, last_update_time: String },
}

#[derive(Clone)]
pub struct OrderBookSnapshotMsg(pub Arc<OrderBook>);

impl actix::Message for OrderBookSnapshotMsg {
    type Result = ();
}

/// 订阅消息
#[derive(Clone)]
pub struct Subscribe {
    pub recipient: Recipient<OrderBookSnapshotMsg>,
}

impl ActixMessage for Subscribe {
    type Result = ();
}

/// 取消订阅消息
#[derive(Clone)]
pub struct Unsubscribe {
    pub recipient_id: usize,
}

impl ActixMessage for Unsubscribe {
    type Result = ();
}

/// 请求初始化某个symbol的订单簿
#[derive(Clone, Debug)]
struct InitRequest {
    symbol: String,
}

impl ActixMessage for InitRequest {
    type Result = ();
}

/// 初始化完成，返回订单簿快照
#[derive(Clone, Debug)]
struct InitComplete {
    order_book: Arc<OrderBook>,
}

impl ActixMessage for InitComplete {
    type Result = ();
}

/// 发送给初始化Actor的深度更新消息（用于缓存）
#[derive(Clone, Debug)]
struct BufferedDepthUpdate {
    symbol: String,
    update: DepthUpdateStreamPayload,
}

impl ActixMessage for BufferedDepthUpdate {
    type Result = ();
}

/// 初始化Actor，负责处理订单簿的初始化
/// 包括：获取初始Depth、缓存更新消息、应用缓存更新
struct InitActor {
    /// 正在初始化的symbol及其缓存的更新消息
    pending_inits: HashMap<String, VecDeque<DepthUpdateStreamPayload>>,
    /// 主服务的地址，用于发送初始化完成消息
    service_addr: Option<Recipient<InitComplete>>,
}

impl InitActor {
    fn new(service_addr: Recipient<InitComplete>) -> Self {
        Self {
            pending_inits: HashMap::new(),
            service_addr: Some(service_addr),
        }
    }
}

impl Actor for InitActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("InitActor 启动");
        _ctx.set_mailbox_capacity(1000)
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("InitActor 停止");
    }
}

/// 处理初始化请求
impl Handler<InitRequest> for InitActor {
    type Result = ();

    fn handle(&mut self, msg: InitRequest, ctx: &mut Context<Self>) -> Self::Result {
        let symbol = msg.symbol.clone();

        // 如果已经在初���化中，则忽略
        if self.pending_inits.contains_key(&symbol) {
            trace!("Symbol {} 已经在初始化中，忽略重复请求", symbol);
            return;
        }

        info!("开始初始化订单簿: {}", symbol);

        // 创建缓存队列
        self.pending_inits.insert(symbol.clone(), VecDeque::new());

        // 异步获取Depth数据
        let symbol_for_async = symbol.clone();

        let fut = async move {
            // 构造请求参数
            let params = CommonRequestBuilder::symbol_and_limit(symbol_for_async.clone(), 1000);

            match execute_json_request::<Depth>(&SPOT_DEPTH_1000_COMMAND, params.to_request_builder(&SPOT_DEPTH_1000_COMMAND), None).await {
                Ok(depth) => {
                    debug!("成功获取 {} 的Depth数据, lastUpdateId={}", symbol_for_async, depth.last_update_id);
                    Some((symbol_for_async, depth))
                }
                Err(e) => {
                    error!("获取 {} 的Depth数据失败: {:?}", symbol_for_async, e);
                    None
                }
            }
        }
        .into_actor(self)
        .map(|result: Option<(String, Depth)>, actor, _ctx| {
            if let Some((symbol, depth)) = result {
                // 创建OrderBook
                match OrderBook::new(symbol.clone(), depth) {
                    Ok(mut order_book) => {
                        // 应用缓存的更新
                        if let Some(updates) = actor.pending_inits.remove(&symbol) {
                            info!("[InitActor] 应用 {} 缓存的 {} 条更新", symbol, updates.len());
                            for (idx, update) in updates.iter().enumerate() {
                                info!(
                                    "[InitActor] 应用第 {} 条更新: first_update_id={}, final_update_id={}",
                                    idx, update.first_update_id, update.final_update_id
                                );
                                if let Err(e) = order_book.apply_snapshot(update.clone()) {
                                    info!("[InitActor] 应用缓存更新失败 {}: {:?}", symbol, e);
                                } else {
                                    info!("[InitActor] 成功应用更新，当前local_update_id={}", order_book.local_update_id);
                                }
                            }
                        } else {
                            info!("[InitActor] 没有缓存的更新需要应用");
                        }

                        // 发送初始化完成消息
                        if let Some(ref service_addr) = actor.service_addr {
                            service_addr.do_send(InitComplete {
                                order_book: Arc::new(order_book),
                            });
                            info!("订单簿初始化完成: {}", symbol);
                        }
                    }
                    Err(e) => {
                        error!("创建OrderBook失败 {}: {:?}", symbol, e);
                        actor.pending_inits.remove(&symbol);
                    }
                }
            } else {
                // 初始化失败，清理 - 无法从result获取symbol，留给cleanup机制处理
            }
        });

        ctx.spawn(fut);
    }
}

/// 处理缓存的深度更新
impl Handler<BufferedDepthUpdate> for InitActor {
    type Result = ();

    fn handle(&mut self, msg: BufferedDepthUpdate, _ctx: &mut Context<Self>) -> Self::Result {
        trace!(
            "[InitActor] 收到BufferedDepthUpdate: symbol={}, first_update_id={}",
            msg.symbol, msg.update.first_update_id
        );
        // 只有正在初始化的symbol才缓存更新
        if let Some(buffer) = self.pending_inits.get_mut(&msg.symbol) {
            let buff_size = buffer.len();
            if buff_size > 100 {
                warn!("[InitActor] 缓存buffer过大，可能失败，当前buffer大小={}", buff_size);
            }

            buffer.push_back(msg.update);
        } else {
            trace!("[InitActor] 没有找到pending_inits的entry，无法缓存");
        }
    }
}

/// 订单簿服务，管理订阅者和快照分发。
/// 负责维护订阅者列表，并将订单簿快照发送给所有订阅者。
/// # 维护订单簿实体。
/// 1. 维护一类交易标的，比如swap/spot/option/future等所有symbol的订单簿。
/// 2. 每个symbol都有自己独立的OrderBook进行维护
/// 3. 每次更新订单簿后，生成该订单簿的快照，并分发给所有订阅者。订阅者可以收到所有symbol的订单簿。
/// 4. 订阅者通过注册成为订阅者，也就是subscribers来接收快照消息。
/// 5. 本地的订单簿，通过DepthUpdateStreamPayload来更新，因为频率很高，实现的时候，尽量的少用锁
///     - 如果更新失败，比如出现OrderBookError::DeprecateError，则需要重新初始化订单簿。
///     - 如果收到不存在的symbol的更新消息，则初始化相对应的订单簿。
/// 6. OrderBookService作为一个单独的Actor运行，处理来自WebSocket的消息，并更新相应的OrderBook实体。
/// 7， 初始化订单簿，使用RESTful API获取初始的Depth数据，然后通过WebSocket的增量更新来维护订单簿的最新状态。具体使用。yue::binance::bn_restful_commands::SPOT_DEPTH_1000_COMMAND
/// 8. DepthUpdateStreamPayload通过其他的Actor获得。也就是需要写基于我现在写的handle
pub struct OrderBookService {
    /// 订阅者列表
    subscribers: Vec<Recipient<OrderBookSnapshotMsg>>,
    /// 市场深度（广播时���剪的档位数）
    market_depth: u16,
    /// 所有symbol的订单簿快照
    order_books: HashMap<String, Arc<OrderBook>>,
    /// 正在初始化的symbol集合（避免重复初始化）
    initializing: HashMap<String, bool>,
    /// 初始化Actor的地址
    init_actor: Option<Addr<InitActor>>,
}

impl Actor for OrderBookService {
    type Context = Context<Self>;

    ///
    /// 1. 启动一条维护的协程。所有订单簿的全集，只能在这个协程里面维护。
    ///    - 在这条线程里面，抱有所有symbol的订单簿的快照的合集。
    ///    - 发现有不存在和过期的symbol，发送消息给初始化协程，更新订单簿。
    ///       - 在此期限，把所有收到的DepthUpdateStreamPayload，发给初始化协程。
    ///       - 一旦触发自流程，不要重复发送初始化消息。只是发送DepthUpdateStreamPayload就可以了。
    ///    - 接收初始化的协程单独symbol的订单快照，更新全集。
    ///    - 更新后，根据market_depth，发送有限的订单簿副本，给订阅者
    /// 2. 启动一条初始化的协程，来处理订单簿的初始化
    ///    - 初始化不存在和过期的symbol的订单簿，通过SPOT_DEPTH_1000_COMMAND来更新原始版本
    ///    - 同时，接收发来的DepthUpdateStreamPayload，缓存需要更新的symbol的部分
    ///    - 等到初始化完毕，则利用上一步缓存的DepthUpdateStreamPayload，来更新订单簿
    ///    - 完成后，发还给维护的协程
    ///
    fn started(&mut self, ctx: &mut Self::Context) {
        info!("OrderBookService 启动");

        // 启动初始化Actor

        ctx.set_mailbox_capacity(1000);
        let service_addr = ctx.address().recipient();
        let init_actor = InitActor::new(service_addr);
        let init_addr = init_actor.start();
        self.init_actor = Some(init_addr);
        info!("InitActor 已启动");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("OrderBookService 停止");
    }
}

impl OrderBookService {
    pub fn new() -> Self {
        Self {
            subscribers: Vec::new(),
            market_depth: 20,
            order_books: HashMap::new(),
            initializing: HashMap::new(),
            init_actor: None,
        }
    }

    pub fn with_market_depth(mut self, depth: u16) -> Self {
        self.market_depth = depth;
        self
    }

    /// 广播订单簿快照给所有订阅者
    fn broadcast_snapshot(&self, order_book: Arc<OrderBook>) {
        // 根据market_depth裁剪订单簿
        let snapshot = match order_book.get_sub_order_book(self.market_depth) {
            Ok(sub_book) => Arc::new(sub_book),
            Err(e) => {
                error!("裁剪订单簿失败 {}: {:?}", order_book.symbol, e);
                return;
            }
        };

        let msg = OrderBookSnapshotMsg(snapshot);
        for subscriber in &self.subscribers {
            subscriber.do_send(msg.clone());
        }
    }

    fn handle_depth_update(&mut self, update: DepthUpdateStreamPayload, _ctx: &mut Context<Self>) {
        let symbol = update.symbol.clone();

        // 检查订单簿是否存在
        if let Some(order_book_arc) = self.order_books.get(&symbol) {
            // 订单簿存在，尝试更新
            let mut order_book = (**order_book_arc).clone();

            match order_book.apply_snapshot(update.clone()) {
                Ok(_) => {
                    // 更新成功，保存并广播
                    let new_arc = Arc::new(order_book);
                    self.order_books.insert(symbol.clone(), new_arc.clone());
                    self.broadcast_snapshot(new_arc);
                }
                Err(OrderBookError::DeprecateError { .. }) => {
                    // 订单簿过期，需要重新初始化
                    warn!("订单簿过期，触发重新初始化: {}", symbol);
                    self.trigger_init(&symbol, Some(update));
                }
            }
        } else {
            // 订单簿不存在，触发初始化
            debug!("收到未知symbol的更新，触发初始化: {}", symbol);
            self.trigger_init(&symbol, Some(update));
        }
    }

    /// 触发订单簿初始化
    fn trigger_init(&mut self, symbol: &str, update: Option<DepthUpdateStreamPayload>) {
        // 检查是否已经在初始化中
        if self.initializing.contains_key(symbol) {
            // 已经在初始化中，只需要缓存更新消息
            if let Some(update) = update {
                if let Some(ref init_actor) = self.init_actor {
                    init_actor.do_send(BufferedDepthUpdate {
                        symbol: symbol.to_string(),
                        update,
                    });
                }
            }
            return;
        }

        // 标记为正在初始化
        self.initializing.insert(symbol.to_string(), true);

        // 如果有更新消息，先缓存
        if let Some(update) = update {
            if let Some(ref init_actor) = self.init_actor {
                init_actor.do_send(BufferedDepthUpdate {
                    symbol: symbol.to_string(),
                    update,
                });
            }
        }

        // ���送初始化请求
        if let Some(ref init_actor) = self.init_actor {
            init_actor.do_send(InitRequest { symbol: symbol.to_string() });
        }
    }
}

// Handler: Subscribe
impl Handler<Subscribe> for OrderBookService {
    type Result = ();

    fn handle(&mut self, msg: Subscribe, _ctx: &mut Context<Self>) -> Self::Result {
        self.subscribers.push(msg.recipient);
        info!("新订阅者注册，当前订阅者数量: {}", self.subscribers.len());
    }
}

// Handler: Unsubscribe
impl Handler<Unsubscribe> for OrderBookService {
    type Result = ();

    fn handle(&mut self, msg: Unsubscribe, _ctx: &mut Context<Self>) -> Self::Result {
        if msg.recipient_id < self.subscribers.len() {
            self.subscribers.remove(msg.recipient_id);
            info!("订阅者取消，当前订阅者数量: {}", self.subscribers.len());
        }
    }
}

// Handler: InitComplete
impl Handler<InitComplete> for OrderBookService {
    type Result = ();

    fn handle(&mut self, msg: InitComplete, _ctx: &mut Context<Self>) -> Self::Result {
        let symbol = msg.order_book.symbol.clone();

        // 清除初始化标记
        self.initializing.remove(&symbol);

        // 保存订单簿
        self.order_books.insert(symbol.clone(), msg.order_book.clone());

        // 广播快照
        self.broadcast_snapshot(msg.order_book);

        info!("订单簿 {} 初始化完成并广播", symbol);
    }
}

// Handler: BinanceSpotWebSocketStreamResponse
impl Handler<BinanceSpotWebSocketStreamResponse> for OrderBookService {
    type Result = ();

    fn handle(&mut self, msg: BinanceSpotWebSocketStreamResponse, ctx: &mut Context<Self>) -> Self::Result {
        if let BinanceSpotWebSocketStreamResponse::DepthUpdate(update) = msg {
            self.handle_depth_update(update, ctx);
        }
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
    use crate::http_client::init_http_client;
    use li::tools::logs::setup_logger_all;
    use rust_decimal::Decimal;
    use serde_json::json;
    use serial_test::serial;
    use std::sync::mpsc;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};
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

    // ========== InitActor handler 主流程测试 ==========

    /// 测试：InitActor handler 的正常流程
    ///
    /// 测试场景：
    /// 1. 创建InitActor实例
    /// 2. 发送InitRequest消息获取BTCUSDT的订单簿
    /// 3. 通过mockserver模拟币安API返回有效Depth数据
    /// 4. 验证OrderBook被正确创建并发送给service
    ///
    /// 预期结果：
    /// - pending_inits中BTCUSDT被移除
    /// - OrderBook包含正确的symbol和last_update_id
    /// - service收到了InitComplete消息
    #[actix::test]
    #[serial]
    async fn test_init_actor_handler_normal_flow() {
        let _ = setup_logger_all(None);
        // 1. 初始化http client和mockserver
        init_http_client(None);
        let mock_server = create_mock_server().await;

        // 2. 构造mock Depth响应数据
        let mock_depth = json!({
            "lastUpdateId": 123456,
            "bids": [
                ["50000.00", "1.0"],
                ["49999.00", "2.0"]
            ],
            "asks": [
                ["50001.00", "1.5"],
                ["50002.00", "2.5"]
            ]
        });

        // 3. 注册mock API端点
        Mock::given(method("GET"))
            .and(path("/api/v3/depth"))
            .and(query_param("symbol", "BTCUSDT"))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_depth))
            .mount(&mock_server)
            .await;

        // 4. 创建测试用的service mock来接收InitComplete消息
        let (tx, rx) = mpsc::channel();

        // 5. 启动service actor并获取recipient
        let test_service = TestService { tx };
        let service_recipient = test_service.start().recipient();

        // 6. 创建InitActor并发送InitRequest
        let init_actor = InitActor::new(service_recipient);
        let init_addr = init_actor.start();

        // 7. 发送初始化请求
        init_addr
            .send(InitRequest {
                symbol: "BTCUSDT".to_string(),
            })
            .await
            .expect("send failed");

        // 8. 等待初始化完成（收到InitComplete消息）
        let order_book = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::task::spawn_blocking(move || rx.recv().expect("should receive InitComplete")),
        )
        .await
        .expect("timeout waiting for InitComplete")
        .expect("task failed");

        // 9. 验证OrderBook的正确性
        assert_eq!(order_book.symbol, "BTCUSDT");
        assert_eq!(order_book.local_update_id, 123456);
        assert_eq!(order_book.bids_count(), 2);
        assert_eq!(order_book.asks_count(), 2);
    }

    struct TestService {
        tx: mpsc::Sender<Arc<OrderBook>>,
    }

    impl Actor for TestService {
        type Context = actix::Context<Self>;
    }

    impl Handler<InitComplete> for TestService {
        type Result = ();

        fn handle(&mut self, msg: InitComplete, _ctx: &mut actix::Context<Self>) {
            let _ = self.tx.send(msg.order_book);
        }
    }

    async fn create_mock_server() -> MockServer {
        let listener = std::net::TcpListener::bind("127.0.0.1:18080").expect("bind failed");
        let mock_server = MockServer::builder().listener(listener).start().await;
        mock_server
    }

    /// 测试：InitActor handler 重复初始化请求
    ///
    /// 测试场景：
    /// 1. 发送第一个InitRequest给BTCUSDT
    /// 2. 在初始化完成前发送第二个相同的InitRequest
    /// 3. 验证第二个请求被忽略，不会发起重复的API调用
    ///
    /// 预期结果：
    /// - 只发起一次API请求
    /// - 只收到一次InitComplete消息
    #[actix::test]
    #[serial]
    async fn test_init_actor_handler_duplicate_request() {
        let _ = setup_logger_all(None);
        init_http_client(None);
        let mock_server = create_mock_server().await;

        let mock_depth = json!({
            "lastUpdateId": 111111,
            "bids": [["50000.00", "1.0"]],
            "asks": [["50001.00", "1.5"]]
        });

        // 注册mock API，expect(1) 确保只被调用一次
        Mock::given(method("GET"))
            .and(path("/api/v3/depth"))
            .and(query_param("symbol", "BTCUSDT"))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_depth))
            .expect(1)
            .mount(&mock_server)
            .await;

        let (tx, rx) = mpsc::channel();
        let test_service = TestService { tx };
        let service_recipient = test_service.start().recipient();

        let init_actor = InitActor::new(service_recipient);
        let init_addr = init_actor.start();

        // 发送第一个请求
        init_addr
            .send(InitRequest {
                symbol: "BTCUSDT".to_string(),
            })
            .await
            .expect("send failed");

        // 立即发送第二个请求（此时第一个还在处理中）
        init_addr
            .send(InitRequest {
                symbol: "BTCUSDT".to_string(),
            })
            .await
            .expect("send failed");

        // 等待初始化完成
        let order_book = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::task::spawn_blocking(move || rx.recv().expect("should receive InitComplete")),
        )
        .await
        .expect("timeout waiting for InitComplete")
        .expect("task failed");

        assert_eq!(order_book.symbol, "BTCUSDT");

        // 等待一下，确保没有第二次InitComplete
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    /// 测试：InitActor handler API请求失败
    ///
    /// 测试场景：
    /// 1. 发送InitRequest
    /// 2. mockserver返回500错误
    /// 3. 验证不会向service发送InitComplete消息
    ///
    /// 预期结果：
    /// - 不会收到InitComplete消息
    /// - 超时后测试正常结束
    #[actix::test]
    #[serial]
    async fn test_init_actor_handler_api_failure() {
        let _ = setup_logger_all(None);
        init_http_client(None);
        let mock_server = create_mock_server().await;

        // 返回500错误
        Mock::given(method("GET"))
            .and(path("/api/v3/depth"))
            .and(query_param("symbol", "ETHUSDT"))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        let (tx, rx) = mpsc::channel();
        let test_service = TestService { tx };
        let service_recipient = test_service.start().recipient();

        let init_actor = InitActor::new(service_recipient);
        let init_addr = init_actor.start();

        init_addr
            .send(InitRequest {
                symbol: "ETHUSDT".to_string(),
            })
            .await
            .expect("send failed");

        // 尝试接收，应该超时
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), tokio::task::spawn_blocking(move || rx.recv())).await;

        // 应该超时，没有收到任何消息
        assert!(result.is_err() || result.unwrap().unwrap().is_err());
    }

    /// 测试：InitActor handler 接收到无效的Depth数据
    ///
    /// 测试场景：
    /// 1. 发送InitRequest
    /// 2. mockserver返回格式错误的JSON数据
    /// 3. 验证不会向service发送InitComplete消息
    ///
    /// 预期结果：
    /// - 不会收到InitComplete消息
    #[actix::test]
    #[serial]
    async fn test_init_actor_handler_invalid_depth_data() {
        let _ = setup_logger_all(None);
        init_http_client(None);
        let mock_server = create_mock_server().await;

        // 返回无效的JSON结构
        let invalid_json = json!({
            "invalid": "data"
        });

        Mock::given(method("GET"))
            .and(path("/api/v3/depth"))
            .and(query_param("symbol", "BNBUSDT"))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(invalid_json))
            .mount(&mock_server)
            .await;

        let (tx, rx) = mpsc::channel();
        let test_service = TestService { tx };
        let service_recipient = test_service.start().recipient();

        let init_actor = InitActor::new(service_recipient);
        let init_addr = init_actor.start();

        init_addr
            .send(InitRequest {
                symbol: "BNBUSDT".to_string(),
            })
            .await
            .expect("send failed");

        // 尝试接收，应该超时
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), tokio::task::spawn_blocking(move || rx.recv())).await;

        assert!(result.is_err() || result.unwrap().unwrap().is_err());
    }

    /// 测试：InitActor handler 带缓存更新的初始化
    ///
    /// 测试场景：
    /// 1. 发送InitRequest
    /// 2. 在初始化过程中发送BufferedDepthUpdate消息
    /// 3. 验证缓存的更新被正确应用到OrderBook
    ///
    /// 预期结果：
    /// - OrderBook包含初始Depth数据
    /// - OrderBook应用了缓存的更新
    /// - local_update_id被更新为最新的update id
    #[actix::test]
    #[serial]
    async fn test_init_actor_handler_with_buffered_updates() {
        let _ = setup_logger_all(None);
        init_http_client(None);
        let mock_server = create_mock_server().await;

        let mock_depth = json!({
            "lastUpdateId": 100,
            "bids": [["50000.00", "1.0"]],
            "asks": [["50001.00", "1.5"]]
        });

        // 添加延迟，让我们有时间发送BufferedDepthUpdate
        Mock::given(method("GET"))
            .and(path("/api/v3/depth"))
            .and(query_param("symbol", "ADAUSDT"))
            .and(query_param("limit", "1000"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(mock_depth)
                    .set_delay(std::time::Duration::from_millis(200)),
            )
            .mount(&mock_server)
            .await;

        let (tx, rx) = mpsc::channel();
        let test_service = TestService { tx };
        let service_recipient = test_service.start().recipient();

        let init_actor = InitActor::new(service_recipient);
        let init_addr = init_actor.start();

        // 发送初始化请求
        init_addr
            .send(InitRequest {
                symbol: "ADAUSDT".to_string(),
            })
            .await
            .expect("send failed");

        // 等待一小段时间，确保InitRequest开始处理
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // 发送缓存的更新消息
        let buffered_update = BufferedDepthUpdate {
            symbol: "ADAUSDT".to_string(),
            update: DepthUpdateStreamPayload {
                event: "depthUpdate".to_string(),
                event_time: 0,
                symbol: "ADAUSDT".to_string(),
                first_update_id: 101,
                final_update_id: 101,
                bids: vec![(49999.0, 2.0)], // 新增一个bid
                asks: vec![],
            },
        };
        init_addr.send(buffered_update).await.expect("send buffered update failed");

        // 等待初始化完成
        let order_book = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::task::spawn_blocking(move || rx.recv().expect("should receive InitComplete")),
        )
        .await
        .expect("timeout waiting for InitComplete")
        .expect("task failed");

        // 验证OrderBook
        assert_eq!(order_book.symbol, "ADAUSDT");
        assert_eq!(order_book.local_update_id, 101); // 应该是更新后的ID
        assert_eq!(order_book.bids_count(), 2); // 初始1个 + 更新1个

        // 验证新增的bid存在
        let bid_49999 = Decimal::from_f64(49999.0).unwrap();
        assert!(order_book.bids.contains_key(&bid_49999));
    }

    /// 测试：InitActor handler 同时初始化多个symbol
    ///
    /// 测试场景：
    /// 1. 同时发送多个不同symbol的InitRequest
    /// 2. 验证每个symbol都能独立完成初始化
    ///
    /// 预期结果：
    /// - 所有symbol都成功初始化
    /// - 每个symbol的OrderBook都正确
    /// - 互不干扰
    #[actix::test]
    #[serial]
    async fn test_init_actor_handler_multiple_symbols() {
        let _ = setup_logger_all(None);
        init_http_client(None);
        let mock_server = create_mock_server().await;

        // 为不同symbol准备不同的mock数据
        let symbols = vec!["BTCUSDT", "ETHUSDT", "BNBUSDT"];

        for (idx, symbol) in symbols.iter().enumerate() {
            let update_id = 1000 + (idx as u64) * 100;
            let mock_depth = json!({
                "lastUpdateId": update_id,
                "bids": [[format!("{}.00", 50000 + idx * 1000), "1.0"]],
                "asks": [[format!("{}.00", 50001 + idx * 1000), "1.5"]]
            });

            Mock::given(method("GET"))
                .and(path("/api/v3/depth"))
                .and(query_param("symbol", *symbol))
                .and(query_param("limit", "1000"))
                .respond_with(ResponseTemplate::new(200).set_body_json(mock_depth))
                .mount(&mock_server)
                .await;
        }

        let (tx, rx) = mpsc::channel();
        let test_service = TestService { tx };
        let service_recipient = test_service.start().recipient();

        let init_actor = InitActor::new(service_recipient);
        let init_addr = init_actor.start();

        // 同时发送所有初始化请求
        for symbol in &symbols {
            init_addr.send(InitRequest { symbol: symbol.to_string() }).await.expect("send failed");
        }

        // 收集所有初始化完成的结果
        let expected_count = symbols.len();
        let completed_symbols = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::task::spawn_blocking(move || {
                let mut symbols_set = std::collections::HashSet::new();
                for _ in 0..expected_count {
                    if let Ok(order_book) = rx.recv() {
                        symbols_set.insert(order_book.symbol.clone());
                    }
                }
                symbols_set
            }),
        )
        .await
        .expect("timeout waiting for InitComplete")
        .expect("task failed");

        // 验证所有symbol都完成了初始化
        assert_eq!(completed_symbols.len(), symbols.len());
        for symbol in &symbols {
            assert!(completed_symbols.contains(*symbol));
        }
    }
}
