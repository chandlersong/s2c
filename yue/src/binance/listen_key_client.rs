use crate::binance::bn_models::common::ListenKeyResponse;
use crate::binance::bn_models::swap_account_stream::BinanceSwapAccountStreamResponse;
use crate::binance::bn_restful_commands::{BNSecurityRequestBuilder, SWAP_LISTEN_KEY_COMMAND, execute_bn_post, execute_bn_put};
use crate::binance::history_data::CommonParam;
use crate::errors::YueError;
use crate::models::RequestInfo;
use crate::websocket::client::{InternalCommand, WebSocketConnection, WebSocketEvent};
use actix::{Actor, AsyncContext, Context, Handler, Message as ActixMessage, Recipient};
use li::tools::SubscribeEvent;
use log::{debug, error, info};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;

/// 手动续期 listen key
#[derive(Clone)]
pub struct RenewListenKey;

impl ActixMessage for RenewListenKey {
    type Result = Result<(), YueError>;
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
pub struct ListenKeyClient {
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
    // 代理 URL（可选）
    proxy: Option<String>,
    // 重连间隔
    reconnect_interval: Duration,
    // 订阅者列表
    subscribers: Vec<Recipient<BinanceSwapAccountStreamResponse>>,
}

impl ListenKeyClient {
    /// 创建新的 ListenKeyClient
    pub fn swap(
        name: &str,
        apply_listen_key_request: RequestInfo,
        renew_listen_key_request: RequestInfo,
        renew_interval_ms: Option<u64>,
        api_key: &str,
        api_secret: &str,
        proxy: Option<String>,
    ) -> Self {
        let actual_renew_interval_ms = renew_interval_ms.unwrap_or(55 * 60 * 1000); // 默认 55 分钟
        Self {
            apply_listen_key_request,
            renew_listen_key_request,
            renew_interval_ms: actual_renew_interval_ms,
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
            name: name.to_string(),
            ws_base_url: "wss://fstream.binance.com/ws/".to_string(),
            proxy,
            reconnect_interval: Duration::from_secs(5),
            subscribers: Vec::new(),
        }
    }

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

        let builder = BNSecurityRequestBuilder::new(api_key.to_string(), api_secret.to_string());
        let create_response = execute_bn_post::<CommonParam, BNSecurityRequestBuilder, ListenKeyResponse>(request_info, None, None, builder)
            .execute()
            .await?;
        Ok(create_response.listen_key)
    }

    /// 发送 HTTP 请求续期 listen key
    async fn renew_current_listen_key(&self) -> Result<(), YueError> {
        let builder = BNSecurityRequestBuilder::new(self.api_key.to_string(), self.api_secret.to_string());
        let renew_response =
            execute_bn_put::<CommonParam, BNSecurityRequestBuilder, ListenKeyResponse>(&SWAP_LISTEN_KEY_COMMAND, None, None, builder)
                .execute()
                .await?;
        println!("renew listen_key is {:?}", renew_response.listen_key);

        info!("成功续期 listen key");
        Ok(())
    }

    ///
    /// 启动一个 WebSocketConnection 连接。
    ///
    async fn run_websocket_connection(url: String, recipient: Recipient<WebSocketEvent>, reconnect_interval: Duration, proxy: Option<String>) {
        // 创建命令通道
        let (command_tx, command_rx) = mpsc::unbounded_channel();

        let message_cache = Arc::new(Mutex::new(Vec::new()));
        if let Err(e) = command_tx.send(InternalCommand::AddSubscriber(recipient)) {
            error!("发送添加订阅者命令失败: {}", e);
        }
        // 启动内部连接管理
        WebSocketConnection::run(url, reconnect_interval, proxy, command_rx, message_cache).await;
    }
}

impl Actor for ListenKeyClient {
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

        let event_reception: Recipient<WebSocketEvent> = ctx.address().recipient();
        let api_key = self.api_key.clone();
        let api_secret = self.api_secret.clone();
        let request_info = self.apply_listen_key_request.clone();
        let base_url = self.ws_base_url.clone();
        let reconnect_interval = self.reconnect_interval.clone();
        let proxy = self.proxy.clone();
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
            let ws_url = format!("{}{}", base_url, listen_key.unwrap());
            Self::run_websocket_connection(ws_url, event_reception, reconnect_interval, proxy).await;
        });
    }
}

impl Handler<RenewListenKey> for ListenKeyClient {
    type Result = actix::ResponseActFuture<Self, Result<(), YueError>>;

    fn handle(&mut self, _msg: RenewListenKey, _ctx: &mut Context<Self>) -> Self::Result {
        let client_self = self.clone();

        let fut = async move { client_self.renew_current_listen_key().await };
        Box::pin(actix::fut::wrap_future::<_, Self>(fut))
    }
}

impl Handler<WebSocketEvent> for ListenKeyClient {
    type Result = ();

    fn handle(&mut self, event: WebSocketEvent, _ctx: &mut Context<Self>) {
        match event {
            WebSocketEvent::Connected(_addr) => {}
            WebSocketEvent::TextMessage(text) => {
                info!("received text message: {}", text);
            }
            WebSocketEvent::BinaryMessage(_) => {}
            WebSocketEvent::Reconnecting => {
                info!("🔄 WebSocket 正在重新连接...");
            }
            WebSocketEvent::Disconnected => {
                info!("bb");
            }
            WebSocketEvent::Error(err) => {
                error!("❌ WebSocket 错误: {}", err);
            }
        }
    }
}

impl Handler<SubscribeEvent<BinanceSwapAccountStreamResponse>> for ListenKeyClient {
    type Result = ();

    fn handle(&mut self, msg: SubscribeEvent<BinanceSwapAccountStreamResponse>, _: &mut Self::Context) -> Self::Result {
        self.subscribers.push(msg.0);
        info!(
            "Subscriber registered for task listen key client:{} Total subscribers: {}",
            self.name,
            self.subscribers.len()
        );
    }
}
