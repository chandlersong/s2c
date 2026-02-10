///
/// 现在把所有和消息处理的东西放在这里，就是为了简单。不然很多类组合，反而很麻烦。
/// TODO
/// 1. 把subscribe和translate给分开。只是基于功能单一原则。但是真实的来说，比较难弄，比如account的转换，需要维护订阅id和账户名的映射关系。
///
///
///
use crate::binance::bn_json_websocket::{
    CommandRequest, StreamCommandRequest, USER_DATA_STREAM_SUBSCRIBE_SIGNATURE, WS_SUBSCRIBE_COMMAND, WS_UNSUBSCRIBE_COMMAND,
};
use crate::binance::bn_models::spot_websocket::BinanceSpotWebSocketResponse;
use crate::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamResponse;
use crate::errors::YueError;
use crate::tools::{SnowyFlakeWrapper, sign_ed25519};
use crate::websocket::client::{SendTextMessage, WebSocketClient, WebSocketEvent};
use crate::websocket::event_bus::WebSocketHandler;
use actix::{Actor, Addr, Handler};
use ed25519_dalek::SigningKey;
use li::actix_jobs::TaskCompletionEvent;
use li::tools::time::unix_time_now_u64_utc;
use log::{error, info, trace};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

pub trait TradingSymbolRefresher {
    fn list_spot(&self) -> Vec<String>;

    fn list_swap(&self) -> Vec<String>;
}

/// Binance 现货websocket处理
///
/// FUTURE：保留websocket订阅kline的能力。
/// 在测试中，发觉订阅Kline和trade等信息在一起，会经常抱错。估计是因为压力太大。
/// 所以就删除通过websocket订阅的部分K线信息就单独订阅。
/// 但是代码保留。以后可以单独起个程序去订阅。属于优化项了
///
/// 最新的可交易symbol通过latest_symbol_refresher的list获得
///
/// # KlineStreamPayload的订阅逻辑
/// Kline订阅的主要问题难点是其它交易对的动态变化。有时候会新增，有时会减少。所以为了保证其它交易对的正确性，需要定期刷新订阅。
/// 但是刷新订阅又是一件非常麻烦的事情。
/// 1. 所有的symbol信息都是源于BinanceDashboard。而这个是定时刷新的。
/// 2. 在连接的时候，就要获得所有的symbol信息，进行订阅。
///
///
/// 1. 从传入的TradingSymbolRefresher来获取需要订阅的交易对列表
///
/// ## 启动时订阅
/// 1. 在启动的时候，收到WebSocketEvent::Connected事件时，调用TradingSymbolRefresher的list方法，获取当前需要订阅的交易对列表。
///
/// ## 定期刷新订阅
/// 监听TaskCompletionEvent事件。然后接收到事件后，进行以下操作：
/// 1. 重新调用TradingSymbolRefresher的list方法，获取最新的交易对列表。。
/// 2. 和现有的交易对做比较。找出新增的和删除的交易对。
/// 2. 发送新增的消息和删除的消息。
pub struct KlineSubscribe {
    refresher: Arc<dyn TradingSymbolRefresher + Send + Sync>,
    kline_interval: String,
    subscribed: RwLock<HashSet<String>>, // 已订阅的symbol集合，使用大写存储便于比较
    last_ws_addr: Option<Addr<WebSocketClient>>,
}

impl Actor for KlineSubscribe {
    type Context = actix::Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("✓ KlineSubscribe actor started");
        _ctx.set_mailbox_capacity(1000);
    }
}

impl Handler<WebSocketEvent> for KlineSubscribe {
    type Result = ();

    fn handle(&mut self, msg: WebSocketEvent, _ctx: &mut Self::Context) -> Self::Result {
        match msg {
            WebSocketEvent::Connected(addr) => {
                self.last_ws_addr = Some(addr.clone());
                if let Some(req) = self.build_initial_subscribe() {
                    match self.send_request(&addr, req) {
                        Ok(_) => info!("✓ K线初始订阅已发送"),
                        Err(e) => error!("❌ 发送K线初始订阅失败: {}", e),
                    }
                }
            }
            _ => {}
        }
    }
}

impl Handler<TaskCompletionEvent> for KlineSubscribe {
    type Result = ();

    fn handle(&mut self, _: TaskCompletionEvent, _: &mut Self::Context) -> Self::Result {
        if let Err(e) = self.refresh() {
            error!("❌ 刷新K线订阅失败: {}", e);
        }
    }
}

impl KlineSubscribe {
    pub fn new(refresher: Arc<dyn TradingSymbolRefresher + Send + Sync>) -> Self {
        Self {
            refresher,
            kline_interval: "5m".to_string(),
            subscribed: RwLock::new(HashSet::new()),
            last_ws_addr: None,
        }
    }

    fn normalize_symbols(&self, symbols: Vec<String>) -> Vec<String> {
        let mut set = HashSet::new();
        for sym in symbols {
            let up = sym.trim().to_uppercase();
            if !up.is_empty() {
                set.insert(up);
            }
        }
        let mut result: Vec<String> = set.into_iter().collect();
        result.sort();
        result
    }

    fn build_streams(&self, symbols: &[String]) -> Vec<String> {
        symbols
            .iter()
            .map(|s| format!("{}@kline_{}", s.to_lowercase(), self.kline_interval))
            .collect()
    }

    fn build_request(&self, method: &str, params: Vec<String>) -> Option<StreamCommandRequest> {
        if params.is_empty() {
            return None;
        }
        let snow_flake = SnowyFlakeWrapper::new();
        Some(StreamCommandRequest {
            method: method.to_string(),
            params,
            id: snow_flake.next_id_u64(),
        })
    }

    pub fn build_initial_subscribe(&self) -> Option<StreamCommandRequest> {
        let latest = self.normalize_symbols(self.refresher.list_spot());
        {
            let mut guard = self.subscribed.write().unwrap();
            guard.clear();
            for sym in &latest {
                guard.insert(sym.clone());
            }
        }
        info!("✓ K线初始订阅交易对: {:?}", latest.len());
        let params = self.build_streams(&latest);
        self.build_request(WS_SUBSCRIBE_COMMAND, params)
    }

    pub fn build_refresh_commands(&self) -> (Option<StreamCommandRequest>, Option<StreamCommandRequest>) {
        let latest = self.normalize_symbols(self.refresher.list_spot());
        let latest_set: HashSet<String> = latest.iter().cloned().collect();

        let (to_add, to_remove) = {
            let current = self.subscribed.read().unwrap();
            let to_add: Vec<String> = latest_set.difference(&*current).cloned().collect();
            let to_remove: Vec<String> = current.difference(&latest_set).cloned().collect();
            (to_add, to_remove)
        };

        {
            let mut guard = self.subscribed.write().unwrap();
            guard.clear();
            for sym in &latest_set {
                guard.insert(sym.clone());
            }
        }
        info!("✓ 新增交易对: {}，减去交易对{}", to_add.len(), to_remove.len());
        let sub_req = self.build_request(WS_SUBSCRIBE_COMMAND, self.build_streams(&to_add));
        let unsub_req = self.build_request(WS_UNSUBSCRIBE_COMMAND, self.build_streams(&to_remove));
        (sub_req, unsub_req)
    }

    fn send_request(&self, addr: &Addr<WebSocketClient>, req: StreamCommandRequest) -> Result<(), YueError> {
        let payload = serde_json::to_string(&req)?;
        addr.try_send(SendTextMessage::new_no_resend(payload))
            .map_err(|e| YueError::new(&format!("发送K线订阅消息失败: {}", e)))
    }

    pub fn refresh(&self) -> Result<(), YueError> {
        if let Some(addr) = &self.last_ws_addr {
            let (sub_req, unsub_req) = self.build_refresh_commands();
            if let Some(req) = sub_req {
                self.send_request(addr, req)?;
                info!("✓ K线新增订阅已发送");
            }
            if let Some(req) = unsub_req {
                self.send_request(addr, req)?;
                info!("✓ K线取消订阅已发送");
            }
        }
        Ok(())
    }
}

pub struct BinanceSpotStreamHandler;

impl WebSocketHandler for BinanceSpotStreamHandler {
    type Output = BinanceSpotWebSocketStreamResponse;

    fn parse_text(&self, text: &str) -> Result<Self::Output, YueError> {
        match BinanceSpotWebSocketStreamResponse::from_text(text) {
            Ok(response) => {
                trace!("✓ 成功解析币安现货行情: {:?}", response);
                Ok(response)
            }
            Err(e) => {
                let preview = if text.len() > 200 {
                    format!("{}...", &text[..200])
                } else {
                    text.to_string()
                };
                Err(YueError::ParseError(format!("解析币安现货行情失败: {}\n消息预览: {}", e, preview)))
            }
        }
    }
}

pub struct AccountWebsocketInfo {
    pub account_name: String,
    pub api_key: String,
    pub private_key: SigningKey,
}

/// Binance 账户流解析器（余额/订单事件）
///
/// ## 设计思路和目的
/// - 订阅 Binance 现货账户数据流（userDataStream），接收余额更新、订单执行报告等事件
/// - 支持多账户并行订阅，每个账户独立维护 listenKey 和订阅 ID
/// - 通过 subscription_id 映射到具体账户，便于后续事件处理和数据入库
///
/// ## 核心职责
/// 1. 在连接建立时（on_connect），为每个账户生成唯一的订阅请求 ID，发送 WebSocket 订阅
/// 2. 维护请求 ID 与账户名的映射（id_to_account）
/// 3. 接收 SubscribeResponse 后，维护 subscription_id 与账户名的映射（subscription_to_account）
/// 4. 解析后续收到的账户事件（OutboundAccountPosition、BalanceUpdate、ExecutionReport）
///    并根据 subscription_id 填充对应的账户名
///
/// - 映射关系可扩展为多维度映射，例如支持账户别名、优先级等
/// - listenKey 管理可独立封装为 ListenKeyManager，支持自动刷新和过期处理
///
/// ## 业务规范
/// - account_name 大小写敏感，应保持一致性
/// - subscription_id 由 Binance 服务器分配，全局唯一
/// - request_id 由本地生成，用于关联订阅请求和响应
pub struct SpotAccountStreamHandler {
    account_info_vec: Vec<AccountWebsocketInfo>,
    id_to_account: RwLock<HashMap<u64, String>>,
    subscription_to_account: RwLock<HashMap<u64, String>>, // 运行期维护订阅ID与账户名关系，读多写少用RwLock
}

impl SpotAccountStreamHandler {
    pub fn new(account_info_vec: Vec<AccountWebsocketInfo>) -> Self {
        Self {
            account_info_vec,
            id_to_account: RwLock::new(HashMap::new()),
            subscription_to_account: RwLock::new(HashMap::new()),
        }
    }

    /// 根据 subscription_id 获取账户名（使用读锁）
    fn get_account_name(&self, subscription_id: u64) -> Result<Option<String>, YueError> {
        let guard = self
            .subscription_to_account
            .read()
            .map_err(|_| YueError::ParseError("解析账户流失败: subscription 映射读锁获取失败".to_string()))?;
        Ok(guard.get(&subscription_id).cloned())
    }
}

impl WebSocketHandler for SpotAccountStreamHandler {
    type Output = BinanceSpotWebSocketResponse;

    ///对于和账户相关的流，各个消息的处理逻辑
    /// 对于账户相关的信息。
    /// OutboundAccountPosition
    /// BalanceUpdate
    /// ExecutionReport
    /// 通过subscription_id找到对应的账户名，并填充到消息中
    ///
    /// SubscribeResponse:
    /// 更新subscription_id和account_name的关系
    ///
    fn parse_text(&self, text: &str) -> Result<Self::Output, YueError> {
        let parsed = match BinanceSpotWebSocketResponse::from_text(text) {
            Ok(response) => response,
            Err(e) => {
                let preview = if text.len() > 200 {
                    format!("{}...", &text[..200])
                } else {
                    text.to_string()
                };
                return Err(YueError::ParseError(format!("解析币安账户流失败: {}\n消息预览: {}", e, preview)));
            }
        };

        match parsed {
            BinanceSpotWebSocketResponse::SubscribeResponse(resp) => {
                if let (Some(id), Some(result)) = (resp.id, resp.result.clone()) {
                    let map = self.id_to_account.read().map_err(|_| YueError::new("获得id_to_account锁失败"))?;
                    if let Some(account_name) = map.get(&id) {
                        let mut guard = self
                            .subscription_to_account
                            .write()
                            .map_err(|_| YueError::ParseError("解析账户流失败: subscription 映射写锁获取失败".to_string()))?;
                        guard.insert(result.subscription_id, account_name.clone());
                        info!("{} 账户订阅成功，subscription_id={}", account_name, result.subscription_id);
                    }
                }
                trace!("✓ 成功解析币安账户订阅响应: {:?}", resp);
                Ok(BinanceSpotWebSocketResponse::SubscribeResponse(resp))
            }
            BinanceSpotWebSocketResponse::OutboundAccountPosition(mut payload) => {
                let account_name = self.get_account_name(payload.subscription_id)?;

                if let Some(name) = account_name {
                    payload.account_name = Some(name);
                    trace!("✓ 成功解析币安账户余额变动: {:?}", payload);
                    Ok(BinanceSpotWebSocketResponse::OutboundAccountPosition(payload))
                } else {
                    Err(YueError::ParseError(format!(
                        "解析币安账户流失败: subscription_id={} 未找到账户映射",
                        payload.subscription_id
                    )))
                }
            }
            BinanceSpotWebSocketResponse::BalanceUpdate(mut payload) => {
                let account_name = self.get_account_name(payload.subscription_id)?;

                if let Some(name) = account_name {
                    payload.account_name = Some(name);
                    trace!("✓ 成功解析币安单资产余额更新: {:?}", payload);
                    Ok(BinanceSpotWebSocketResponse::BalanceUpdate(payload))
                } else {
                    Err(YueError::ParseError(format!(
                        "解析币安账户流失败: subscription_id={} 未找到账户映射",
                        payload.subscription_id
                    )))
                }
            }
            BinanceSpotWebSocketResponse::ExecutionReport(mut payload) => {
                let account_name = self.get_account_name(payload.subscription_id)?;

                if let Some(name) = account_name {
                    payload.account_name = Some(name);
                    trace!("✓ 成功解析币安订单执行报告: {:?}", payload);
                    Ok(BinanceSpotWebSocketResponse::ExecutionReport(payload))
                } else {
                    Err(YueError::ParseError(format!(
                        "解析币安账户流失败: subscription_id={} 未找到账户映射",
                        payload.subscription_id
                    )))
                }
            }
        }
    }

    /// 这里通过account_info_vec，给websocket client发送信息。订阅账户信息。
    /// 然后把id和account_name的映射关系存储起来，方便后续消息处理时使用。
    ///
    fn on_connect(&self, addr: &Addr<WebSocketClient>) -> Result<(), YueError> {
        {
            // 每次重连前清空旧的请求ID映射
            let mut map = self
                .id_to_account
                .write()
                .map_err(|e| YueError::new(&format!("获取id_to_account写锁失败: {}", e)))?;
            map.clear();
        }

        let snow_flake = SnowyFlakeWrapper::new();
        for account_info in &self.account_info_vec {
            // 1. 生成唯一的 request ID
            let request_id = snow_flake.next_id_u64();
            let now = unix_time_now_u64_utc();
            let payload = format!("apiKey={}&timestamp={}", account_info.api_key, now);
            let mut key = account_info.private_key.clone();
            let signature = sign_ed25519(payload, &mut key)?;
            let param = HashMap::from([
                ("signature".to_string(), signature),
                ("apiKey".to_string(), account_info.api_key.clone()),
                ("timestamp".to_string(), now.to_string()),
            ]);

            // 3. 构建 WebSocket 订阅请求
            let command = CommandRequest {
                method: USER_DATA_STREAM_SUBSCRIBE_SIGNATURE.to_string(),
                params: param,
                id: request_id.clone(),
            };

            // 5. 发送 WebSocket 消息
            let message = SendTextMessage::new_no_resend(serde_json::to_string(&command)?);
            match addr.try_send(message) {
                Ok(_) => {
                    info!("✓ 已发送账户 {} 的订阅请求 (id={})", account_info.account_name, request_id);
                }
                Err(e) => {
                    error!("❌ 无法发送账户 {} 的订阅请求: {}", account_info.account_name, e);
                    continue;
                }
            }
            {
                let mut map = match self.id_to_account.write() {
                    Ok(m) => m,
                    Err(e) => {
                        error!("无法获得 id_to_account 写锁: {}", e);
                        continue;
                    }
                };
                map.insert(request_id, account_info.account_name.clone());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::prelude::ToPrimitive;

    #[test]
    fn test_parse_trade_message() {
        let parser = BinanceSpotStreamHandler {};
        let trade_json = r#"{
            "e":"trade",
            "E":1234567890,
            "s":"BTCUSDT",
            "t":123456,
            "p":"40000.00",
            "q":"1.0",
            "T":1234567890,
            "m":false,
            "M":false
        }"#;

        let result = parser.parse_text(trade_json);
        assert!(result.is_ok(), "Failed to parse trade message: {:?}", result);

        match result.unwrap() {
            BinanceSpotWebSocketStreamResponse::Trade(trade) => {
                assert_eq!(trade.symbol, "BTCUSDT");
                assert_eq!(trade.trade_id, 123456);
                assert!((trade.price.to_f64().unwrap() - 40000.00).abs() < 0.01);
            }
            _ => panic!("Expected Trade variant"),
        }
    }

    #[test]
    fn test_parse_invalid_json() {
        let parser = BinanceSpotStreamHandler;
        let invalid_json = r#"{\"invalid\": json}"#;

        let result = parser.parse_text(invalid_json);
        assert!(result.is_err(), "Should fail on invalid JSON");
    }

    #[test]
    fn test_parse_empty_string() {
        let parser = BinanceSpotStreamHandler;
        let result = parser.parse_text("");
        assert!(result.is_err(), "Should fail on empty string");
    }

    #[test]
    fn test_parse_outbound_account_position() {
        let parser = SpotAccountStreamHandler::new(vec![]);

        // 手动插入 id -> account_name 映射
        {
            let mut map = parser.id_to_account.write().unwrap();
            map.insert(42, "acc_test".to_string());
        }

        let subscribe_json = r#"{"id":42,"status":200,"result":{"subscriptionId":123}}"#;
        parser.parse_text(subscribe_json).expect("subscribe should build mapping");
        let json = r#"{
            "subscriptionId": 123,
            "event": {
                "e":"outboundAccountPosition",
                "E":1690000000000,
                "u":1690000000000,
                "B":[
                    {
                        "a":"BTC",
                        "f":"1.5",
                        "l":"0.5"
                    },
                    {
                        "a":"USDT",
                        "f":"50000.0",
                        "l":"0.0"
                    }
                ]
            }
        }"#;

        let result = parser.parse_text(json);
        assert!(result.is_ok(), "Failed to parse account position: {:?}", result);
        match result.unwrap() {
            BinanceSpotWebSocketResponse::OutboundAccountPosition(payload) => {
                assert_eq!(payload.subscription_id, 123);
                assert_eq!(payload.event.event, "outboundAccountPosition");
                assert_eq!(payload.event.balances.len(), 2);
                assert_eq!(payload.account_name.as_deref(), Some("acc_test"));
            }
            _ => panic!("Expected OutboundAccountPosition variant"),
        }
    }

    #[test]
    fn test_parse_subscribe_response() {
        let parser = SpotAccountStreamHandler::new(vec![]);
        let json = r#"{"id":1,"status":200,"result":{"subscriptionId":12345}}"#;
        let result = parser.parse_text(json);
        assert!(result.is_ok(), "Failed to parse subscribe response: {:?}", result);
        match result.unwrap() {
            BinanceSpotWebSocketResponse::SubscribeResponse(resp) => {
                assert_eq!(resp.status, Some(200));
                assert_eq!(resp.result.unwrap().subscription_id, 12345);
            }
            _ => panic!("Expected SubscribeResponse variant"),
        }
    }

    #[test]
    fn test_account_parser_invalid_json() {
        let parser = SpotAccountStreamHandler::new(vec![]);
        let invalid = "{";
        let result = parser.parse_text(invalid);
        assert!(result.is_err(), "Account parser should fail on invalid json");
    }

    #[test]
    fn test_account_event_with_account_mapping() {
        let parser = SpotAccountStreamHandler::new(vec![]);

        // 手动插入 id -> account_name 映射
        {
            let mut map = parser.id_to_account.write().unwrap();
            map.insert(1, "acc_a".to_string());
        }

        let subscribe_json = r#"{"id":1,"status":200,"result":{"subscriptionId":999}}"#;
        parser.parse_text(subscribe_json).expect("subscribe response should parse");

        let account_event = r#"{
            "subscriptionId": 999,
            "event": {
                "e":"outboundAccountPosition",
                "E":1690000000000,
                "u":1690000000000,
                "B":[{"a":"BTC","f":"1.5","l":"0.5"}]
            }
        }"#;

        let parsed = parser.parse_text(account_event).expect("account event should parse");
        match parsed {
            BinanceSpotWebSocketResponse::OutboundAccountPosition(p) => {
                assert_eq!(p.account_name.as_deref(), Some("acc_a"));
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn test_account_event_missing_subscription_mapping() {
        let parser = SpotAccountStreamHandler::new(vec![]);
        let account_event = r#"{
            "subscriptionId": 321,
            "event": {
                "e":"outboundAccountPosition",
                "E":1690000000000,
                "u":1690000000000,
                "B":[{"a":"BTC","f":"1.5","l":"0.5"}]
            }
        }"#;

        let parsed = parser.parse_text(account_event);
        assert!(parsed.is_err());
    }

    #[test]
    fn test_on_connect_builds_id_mapping() {
        // 创建带有账户信息的处理器
        // 使用固定的签名密钥用于测试
        let signing_key_bytes = [0u8; 32];
        let account_infos = vec![
            AccountWebsocketInfo {
                account_name: "acc_test_1".to_string(),
                api_key: "key1".to_string(),
                private_key: ed25519_dalek::SigningKey::from_bytes(&signing_key_bytes),
            },
            AccountWebsocketInfo {
                account_name: "acc_test_2".to_string(),
                api_key: "key2".to_string(),
                private_key: ed25519_dalek::SigningKey::from_bytes(&signing_key_bytes),
            },
        ];

        let handler = SpotAccountStreamHandler::new(account_infos);

        // 验证初始状态：id_to_account 为空
        {
            let map = handler.id_to_account.read().unwrap();
            assert_eq!(map.len(), 0, "初始状态下 id_to_account 应为空");
        }
    }

    struct MockRefresher {
        symbols: RwLock<Vec<String>>,
    }

    impl MockRefresher {
        fn new(symbols: Vec<&str>) -> Self {
            Self {
                symbols: RwLock::new(symbols.iter().map(|s| s.to_string()).collect()),
            }
        }

        fn set(&self, symbols: Vec<&str>) {
            let mut guard = self.symbols.write().unwrap();
            guard.clear();
            guard.extend(symbols.into_iter().map(|s| s.to_string()));
        }
    }

    impl TradingSymbolRefresher for MockRefresher {
        fn list_spot(&self) -> Vec<String> {
            self.symbols.read().unwrap().clone()
        }

        fn list_swap(&self) -> Vec<String> {
            vec![]
        }
    }

    #[test]
    fn test_kline_initial_subscribe_request_building() {
        let refresher = Arc::new(MockRefresher::new(vec!["BTCUSDT", "ETHUSDT"]));
        let kline = KlineSubscribe::new(refresher);

        let req = kline.build_initial_subscribe().expect("初始订阅请求应生成");

        assert_eq!(req.method, WS_SUBSCRIBE_COMMAND);
        assert_eq!(req.params.len(), 2);
        assert!(req.params.contains(&"btcusdt@kline_5m".to_string()));
        assert!(req.params.contains(&"ethusdt@kline_5m".to_string()));

        let state = kline.subscribed.read().unwrap();
        assert_eq!(state.len(), 2);
    }

    #[test]
    fn test_kline_refresh_diff_building() {
        let refresher = Arc::new(MockRefresher::new(vec!["BTCUSDT"]));
        let kline = KlineSubscribe::new(refresher.clone());

        kline.build_initial_subscribe();

        refresher.set(vec!["BTCUSDT", "BNBUSDT"]);
        let (sub_req, unsub_req) = kline.build_refresh_commands();
        assert!(unsub_req.is_none());
        let sub_req = sub_req.expect("应该有新增订阅请求");
        assert_eq!(sub_req.params, vec!["bnbusdt@kline_5m".to_string()]);

        refresher.set(vec!["BNBUSDT"]);
        let (sub_req, unsub_req) = kline.build_refresh_commands();
        assert!(sub_req.is_none());
        let unsub_req = unsub_req.expect("应该有取消订阅请求");
        assert_eq!(unsub_req.params, vec!["btcusdt@kline_5m".to_string()]);
    }
}
