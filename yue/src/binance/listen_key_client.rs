use crate::binance::bn_json_websocket::{PORTFOLIO_MARGIN_STREAM_WEBSOCKET, SWAP_WEBSOCKET};
use crate::binance::bn_models::common::{AccountData, ListenKeyResponse, PortfolioSpotOrderData, PortfolioSwapOrderData, SwapOrderData};
use crate::binance::bn_models::portfolio_account_websocket::BinancePortfolioWebSocketStreamResponse;
use crate::binance::bn_models::swap_account_stream::BinanceSwapAccountStreamResponse;
use crate::binance::bn_restful_commands::{PAPI_LISTEN_KEY_COMMAND, SWAP_LISTEN_KEY_COMMAND, execute_json_request};
use crate::binance::http_client::{BinanceSecurityInfo, BinanceSecurityType};
use crate::errors::YueError;
use crate::http_client::get_http_client;
use crate::models::RequestInfo;
use actix::{Actor, AsyncContext, Context, Handler, Message as ActixMessage, Recipient};
use li::errors::LiError;
use li::tools::SubscribeEvent;
use li::websocket::client::{CommandMessage, ConnectionCommand, WebSocketConnection, WebSocketEvent};
use li::websocket::models::WebSocketMessage;
use log::{debug, error, info};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::mpsc;
/// 手动续期 listen key
#[derive(Clone)]
pub struct RenewListenKey;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::sleep;

impl ActixMessage for RenewListenKey {
    type Result = Result<(), YueError>;
}

#[derive(Clone)]
pub struct NormalAccountAssignName {
    recipient: Vec<Recipient<SwapOrderData>>,
    account_name: String,
}

impl NormalAccountAssignName {
    pub fn new(account_name: &str) -> Self {
        Self {
            recipient: Vec::new(),
            account_name: account_name.to_string(),
        }
    }
}

impl Actor for NormalAccountAssignName {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.set_mailbox_capacity(1000);
    }
}

impl Handler<SubscribeEvent<SwapOrderData>> for NormalAccountAssignName {
    type Result = ();
    fn handle(&mut self, msg: SubscribeEvent<SwapOrderData>, _ctx: &mut Self::Context) -> Self::Result {
        info!("AssignAccountNameActor add new subscribe: {}", self.account_name);
        self.recipient.push(msg.0);
    }
}

impl Handler<BinanceSwapAccountStreamResponse> for NormalAccountAssignName {
    type Result = ();

    fn handle(&mut self, msg: BinanceSwapAccountStreamResponse, _ctx: &mut Self::Context) -> Self::Result {
        match msg {
            BinanceSwapAccountStreamResponse::OrderTradeUpdate(order) => {
                let message = AccountData::new(self.account_name.as_ref(), order);
                for recipient in &self.recipient {
                    recipient.do_send(message.clone());
                }
            }
            _ => {}
        }
    }
}

#[derive(Clone)]
pub struct PortfolioAccountAssignName {
    recipient: Vec<Recipient<PortfolioSpotOrderData>>,
    swap_recipient: Vec<Recipient<PortfolioSwapOrderData>>,
    account_name: String,
}

impl PortfolioAccountAssignName {
    pub fn new(account_name: &str) -> Self {
        Self {
            recipient: Vec::new(),
            swap_recipient: vec![],
            account_name: account_name.to_string(),
        }
    }
}

impl Actor for PortfolioAccountAssignName {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.set_mailbox_capacity(1000);
    }
}

impl Handler<SubscribeEvent<PortfolioSpotOrderData>> for PortfolioAccountAssignName {
    type Result = ();
    fn handle(&mut self, msg: SubscribeEvent<PortfolioSpotOrderData>, _ctx: &mut Self::Context) -> Self::Result {
        info!("AssignAccountNameActor add new subscribe: {}", self.account_name);
        self.recipient.push(msg.0);
    }
}

impl Handler<SubscribeEvent<PortfolioSwapOrderData>> for PortfolioAccountAssignName {
    type Result = ();
    fn handle(&mut self, msg: SubscribeEvent<PortfolioSwapOrderData>, _ctx: &mut Self::Context) -> Self::Result {
        info!("AssignAccountNameActor add new subscribe: {}", self.account_name);
        self.swap_recipient.push(msg.0);
    }
}

impl Handler<BinancePortfolioWebSocketStreamResponse> for PortfolioAccountAssignName {
    type Result = ();

    fn handle(&mut self, msg: BinancePortfolioWebSocketStreamResponse, _ctx: &mut Self::Context) -> Self::Result {
        match msg {
            BinancePortfolioWebSocketStreamResponse::ExecutionReport(report) => {
                let message = AccountData::new(self.account_name.as_ref(), report);
                for recipient in &self.recipient {
                    recipient.do_send(message.clone());
                }
            }
            BinancePortfolioWebSocketStreamResponse::OrderTradeUpdate(order_update) => {
                let message = AccountData::new(self.account_name.as_ref(), order_update);
                for recipient in &self.swap_recipient {
                    recipient.do_send(message.clone());
                }
            }
            _ => {}
        }
    }
}

///
/// # 说明
/// 根据币安的说明。[币本位账户信息流](https://developers.binance.com/docs/zh-CN/derivatives/coin-margined-futures/user-data-streams)
/// 在通过websocket监听swap等account变化的时候。大致步骤是
/// 1. 通过REST接口获取listen key
/// 2. 通过websocket，带上listen key去监听。
///
/// 而对于listen key的维护，币安的说明是：
/// 1. listen key的有效期为60分钟，过期后需要延长或者更新listen key
/// 2. 单个连接只能有一个listen key，如果需要监听多个账户或者多个交易所，需要建立多个连接。
/// 3. 单个链接，24小时后会断开重连。
///

///
/// # 设计思路
/// 1. 一个client，对应一个用户的websocket的定义方式。
/// 2. 这个client的职责是维护listen key的状态，定时更新listen key，提供接口获取listen key。
/// 3. 内部使用ListenKeyConnection处理WebSocket连接，逻辑参考WebSocketConnection。
///
/// # 扩展点
/// - 可以加入失败重试机制和指数退避
/// - 可以加入 listen key 过期前提前续期的策略
/// - 可以支持多个订阅者监听 listen key 变化事件
///
#[derive(Clone)]
pub struct ListenKeyClient<M: WebSocketMessage> {
    // 获取新 listen key 的请求信息
    pub apply_listen_key_request: RequestInfo,
    // 续期 listen key 的请求信息
    pub renew_listen_key_request: RequestInfo,
    // 续期间隔（毫秒），默认 3600000ms = 60 分钟
    pub renew_interval_ms: u64,
    api_key: String,
    api_secret: String,
    name: String,
    // WebSocket 基础 URL，例如 wss://fstream.binance.com/ws
    ws_base_url: String,
    // 代理 URL（可选)
    proxy: Option<String>,
    // 重连间隔
    reconnect_interval: Duration,

    command_tx: Option<mpsc::UnboundedSender<ConnectionCommand<Self, M>>>,
}

impl ListenKeyClient<BinanceSwapAccountStreamResponse> {
    pub fn swap(name: &str, renew_interval_ms: Option<u64>, api_key: &str, api_secret: &str, proxy: Option<String>) -> Self {
        let actual_renew_interval_ms = renew_interval_ms.unwrap_or(55 * 60 * 1000); // 默认 55 分钟
        Self {
            apply_listen_key_request: SWAP_LISTEN_KEY_COMMAND.clone(),
            renew_listen_key_request: SWAP_LISTEN_KEY_COMMAND.clone(),
            renew_interval_ms: actual_renew_interval_ms,
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
            name: name.to_string(),
            ws_base_url: SWAP_WEBSOCKET.to_string(),
            proxy,
            reconnect_interval: Duration::from_secs(5),
            command_tx: None,
        }
    }
}

impl ListenKeyClient<BinancePortfolioWebSocketStreamResponse> {
    pub fn portfolio(name: &str, renew_interval_ms: Option<u64>, api_key: &str, api_secret: &str, proxy: Option<String>) -> Self {
        let actual_renew_interval_ms = renew_interval_ms.unwrap_or(55 * 60 * 1000); // 默认 55 分钟
        Self {
            apply_listen_key_request: PAPI_LISTEN_KEY_COMMAND.clone(),
            renew_listen_key_request: PAPI_LISTEN_KEY_COMMAND.clone(),
            renew_interval_ms: actual_renew_interval_ms,
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
            name: name.to_string(),
            ws_base_url: PORTFOLIO_MARGIN_STREAM_WEBSOCKET.to_string(),
            proxy,
            reconnect_interval: Duration::from_secs(5),
            command_tx: None,
        }
    }
}

impl<M: WebSocketMessage> ListenKeyClient<M> {
    /// 创建新的 ListenKeyClient

    /// 设置 WebSocket 基础 URL
    pub fn with_ws_base_url(mut self, url: impl Into<String>) -> Self {
        self.ws_base_url = url.into();
        self
    }

    /// 设置代理 URL
    pub fn with_proxy(mut self, proxy: impl Into<String>) -> Self {
        self.proxy = Some(proxy.into());
        self
    }

    /// 从环境变量设置代理，优先级: WS_PROXY -> HTTPS_PROXY -> HTTP_PROXY
    pub fn with_env_proxy(mut self) -> Self {
        let proxy = std::env::var("WS_PROXY")
            .or_else(|_| std::env::var("HTTPS_PROXY"))
            .or_else(|_| std::env::var("HTTP_PROXY"))
            .ok()
            .and_then(|p| if p.is_empty() { None } else { Some(p) });

        if let Some(proxy_url) = proxy {
            info!("从环境变量读取代理: {}", proxy_url);
            self.proxy = Some(proxy_url);
        }
        self
    }

    /// 设置重连间隔
    pub fn with_reconnect_interval(mut self, interval: Duration) -> Self {
        self.reconnect_interval = interval;
        self
    }

    /// 发送 HTTP 请求获取新的 listen key
    async fn fetch_new_listen_key(api_key: &str, api_secret: &str, request_info: &RequestInfo) -> Result<String, YueError> {
        info!("正在获取新的 listen key...");
        let client = get_http_client();
        let rb = client.post(request_info.as_ref().as_str());
        let security_info = BinanceSecurityInfo::new(api_key, api_secret, BinanceSecurityType::HMAC);
        let create_response = execute_json_request::<ListenKeyResponse>(request_info, rb, Some(security_info)).await?;
        Ok(create_response.listen_key)
    }

    /// 发送 HTTP 请求续期 listen key
    async fn renew_current_listen_key(&self) -> Result<(), YueError> {
        info!("正在获取新的 listen key...");
        let client = get_http_client();
        let rb = client.put(self.renew_listen_key_request.as_str());
        let security_info = BinanceSecurityInfo::new(&self.api_key, &self.api_secret, BinanceSecurityType::HMAC);
        let renew_response = execute_json_request::<ListenKeyResponse>(&SWAP_LISTEN_KEY_COMMAND, rb, Some(security_info)).await?;
        println!("renew listen_key is {:?}", renew_response.listen_key);
        info!("成功续期 listen key");
        Ok(())
    }

    ///
    /// 启动一个 WebSocketConnection 连接。
    ///
    async fn run_websocket_connection(
        url: String,
        reconnect_interval: Duration,
        proxy: Option<String>,
        command_rx: UnboundedReceiver<ConnectionCommand<ListenKeyClient<M>, M>>,
    ) {
        // 创建命令通道

        let message_cache = Arc::new(Mutex::new(Vec::new()));
        // 启动内部连接管理
        WebSocketConnection::<Self, M>::run(url, reconnect_interval, proxy, command_rx, message_cache).await;
    }
}

impl<M: WebSocketMessage> Actor for ListenKeyClient<M> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("ListenKeyClient started, 续期间隔: {} ms", self.renew_interval_ms);
        info!("开始监听账户,账户为: {} ", self.name);

        // 启动定时续期任务
        let addr = ctx.address();
        let interval_duration = Duration::from_millis(self.renew_interval_ms);
        tokio::spawn(async move {
            loop {
                sleep(interval_duration).await;
                match addr.try_send(RenewListenKey) {
                    Ok(_) => {
                        debug!("续期任务已发送");
                    }
                    Err(e) => {
                        error!("发送续期任务失败: {}", e);
                    }
                }
            }
        });

        let api_key = self.api_key.clone();
        let api_secret = self.api_secret.clone();
        let request_info = self.apply_listen_key_request.clone();
        let base_url = self.ws_base_url.clone();
        let reconnect_interval = self.reconnect_interval.clone();
        let proxy = self.proxy.clone();
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        self.command_tx = Some(command_tx.clone());
        tokio::spawn(async move {
            /* FUTURE: listen key 刷新
             * 因为根据最新的说法，listen key 的有效期是 60 分钟，所以续期的间隔设置为 55 分钟，留出一些余量。
             * 而我在群里面问过，其实key是不会变的。所以来说这里就没有做任何刷新的逻辑了。
             *  后续如果有需要，可以在续期的时候，判断一下是否需要刷新 listen key，如果需要刷新，就重新获取 listen key，并重启 WebSocket 连接。
             */
            let listen_key = Self::fetch_new_listen_key(api_key.as_ref(), api_secret.as_ref(), &request_info).await;
            if let Err(e) = listen_key {
                error!("获取 listen key 失败: {}", e);
                return;
            }
            let ws_url = format!("{}/{}", base_url, listen_key.unwrap());
            Self::run_websocket_connection(ws_url, reconnect_interval, proxy, command_rx).await;
        });
    }
}

impl<M: WebSocketMessage> Handler<RenewListenKey> for ListenKeyClient<M> {
    type Result = actix::ResponseActFuture<Self, Result<(), YueError>>;

    fn handle(&mut self, _msg: RenewListenKey, _ctx: &mut Context<Self>) -> Self::Result {
        let client_self = self.clone();

        let fut = async move { client_self.renew_current_listen_key().await };
        Box::pin(actix::fut::wrap_future::<_, Self>(fut))
    }
}

impl<M: WebSocketMessage> Handler<WebSocketEvent> for ListenKeyClient<M> {
    type Result = ();

    fn handle(&mut self, event: WebSocketEvent, _ctx: &mut Context<Self>) {
        match event {
            WebSocketEvent::Connected(_addr) => {
                info!("✅ WebSocket {} 已连接:", self.name);
            }
            WebSocketEvent::Reconnecting => {
                info!("🔄 WebSocket {} 正在重新连接...", self.name);
            }
            WebSocketEvent::Disconnected => {
                info!("❌ WebSocket {} 断开..", self.name);
            }
            WebSocketEvent::Error(err) => {
                error!("❌ WebSocket 错误: {}", err);
            }
        }
    }
}

impl<M: WebSocketMessage> Handler<CommandMessage> for ListenKeyClient<M> {
    type Result = Result<(), LiError>;

    ///
    /// 因为Listen key默认不支持外部发消息，所以这里直接 panic，后续如果有需要，可以在这里加入对外部命令的处理逻辑。
    ///
    fn handle(&mut self, _msg: CommandMessage, _ctx: &mut Context<Self>) -> Self::Result {
        panic!("ListenKeyClient 不支持 CommandMessage");
    }
}

impl<M: WebSocketMessage> Handler<SubscribeEvent<M>> for ListenKeyClient<M> {
    type Result = ();
    fn handle(&mut self, msg: SubscribeEvent<M>, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(ref tx) = self.command_tx {
            tx.send(ConnectionCommand::AddMessageSubscriber(msg.0))
                .map_err(|e| error!("添加订阅者失败: {}", e))
                .ok();
            info!("订阅请求已发送");
        }
    }
}
