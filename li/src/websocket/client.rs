use crate::errors::LiError;
use crate::tools::SubscribeEvent;
use crate::websocket::models::WebSocketMessage;
use actix::Message as ActixMessage;
use actix::{Actor, Addr, AsyncContext, Context, Handler, Recipient};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, trace, warn};
use std::fmt::Debug;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

/// WebSocket 事件，发送给订阅者
#[derive(Clone, Debug, ActixMessage)]
#[rtype(result = "()")]
pub enum WebSocketEvent {
    /// 连接成功，携带 WebSocketClient 的地址
    Connected(Recipient<CommandMessage>),
    /// 连接断开
    Disconnected,
    /// 重连中
    Reconnecting,
    /// 错误
    Error(String),
}

///
/// 客户端给服务器端发送的消息。
/// Binary暂时先不管，也就是一个new的区别
///
#[derive(Clone, Debug, ActixMessage)]
#[rtype(result = "Result<(), LiError>")]
pub enum CommandMessage {
    Text(String, bool),
    Binary(Vec<u8>, bool),
}

impl CommandMessage {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into(), true)
    }

    pub fn text_no_resend(text: impl Into<String>) -> Self {
        Self::Text(text.into(), false)
    }

    ///
    /// 返回是WsMessage，和是否要重发
    ///
    pub fn to_ws_message(&self) -> (WsMessage, bool) {
        match self {
            CommandMessage::Text(txt, resend) => (WsMessage::Text(txt.clone().into()), *resend),
            CommandMessage::Binary(data, resend) => (WsMessage::Binary(data.clone().into()), resend.clone()),
        }
    }
}

/// 内部命令，用于 Actor 和 Connection 之间通信
pub enum ConnectionCommand<C: Actor, M: WebSocketMessage> {
    /// 添加订阅者
    AddEventSubscriber(Recipient<WebSocketEvent>),

    AddMessageSubscriber(Recipient<M>),
    /// 发送 WebSocket 消息
    SendMessage(WsMessage),
    /// 设置 Client 地址（用于传递给订阅者）
    SetClientAddr(Addr<C>),
}

/// WebSocket 客户端 Actor
/// 负责：订阅管理、消息发送的外部接口
pub struct WebSocketClient<M: WebSocketMessage> {
    url: String,
    reconnect_interval: Duration,
    proxy: Option<String>,
    /// 向内部连接发送命令
    command_tx: Option<mpsc::UnboundedSender<ConnectionCommand<Self, M>>>,
    /// 消息缓存
    /// 这个缓存，放在connection里面可能更加好一点。但是放在client里面，主要是为了以后的更改和去除。
    command_cache: Arc<Mutex<Vec<WsMessage>>>,
}

impl<M: WebSocketMessage> WebSocketClient<M> {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            reconnect_interval: Duration::from_secs(5),
            proxy: None,
            command_tx: None,
            command_cache: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 从环境变量创建 WebSocketClient，优先级: WS_PROXY -> HTTPS_PROXY -> HTTP_PROXY
    pub fn new_with_env_proxy(url: impl Into<String>) -> Self {
        let proxy = std::env::var("WS_PROXY")
            .or_else(|_| std::env::var("HTTPS_PROXY"))
            .or_else(|_| std::env::var("HTTP_PROXY"))
            .ok()
            .and_then(|p| if p.is_empty() { None } else { Some(p) });

        let mut client = Self::new(url);
        if let Some(proxy_url) = proxy {
            info!("从环境变量读取代理: {}", proxy_url);
            client.proxy = Some(proxy_url);
        }
        client
    }

    pub fn with_proxy(mut self, proxy: impl Into<String>) -> Self {
        self.proxy = Some(proxy.into());
        self
    }

    pub fn with_reconnect_interval(mut self, interval: Duration) -> Self {
        self.reconnect_interval = interval;
        self
    }

    /// 添加消息到缓存
    pub fn add_to_cache(&self, event: WsMessage) {
        let mut cache = self.command_cache.lock().unwrap();
        cache.push(event);
    }

    /// 清空缓存
    pub fn clear_command_cache(&self) {
        self.command_cache.lock().unwrap().clear();
    }
}

impl<M: WebSocketMessage> Actor for WebSocketClient<M> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let url = self.url.clone();
        let reconnect_interval = self.reconnect_interval;
        let proxy = self.proxy.clone();

        // 创建命令通道
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        self.command_tx = Some(command_tx.clone());
        info!("WebSocketClient started, connecting to {}", url);

        let message_cache = self.command_cache.clone();

        // 获取当前 Actor 地址
        let client_addr = ctx.address();

        // 将地址发送到连接管理器
        let _ = command_tx.send(ConnectionCommand::SetClientAddr(client_addr));

        // 启动内部连接管理
        tokio::spawn(async move {
            WebSocketConnection::run(url, reconnect_interval, proxy, command_rx, message_cache).await;
        });
    }
}

impl<M: WebSocketMessage> Handler<CommandMessage> for WebSocketClient<M> {
    type Result = Result<(), LiError>;

    fn handle(&mut self, msg: CommandMessage, _ctx: &mut Context<Self>) -> Self::Result {
        if let Some(ref tx) = self.command_tx {
            let (message, resend) = msg.to_ws_message();
            if resend {
                self.add_to_cache(message.clone());
            }
            tx.send(ConnectionCommand::SendMessage(message))
                .map_err(|e| LiError::CustomError(format!("发送文本消息失败: {}", e)))
        } else {
            Err(LiError::CustomError("WebSocket 客户端未初始化".to_string()))
        }
    }
}

impl<M: WebSocketMessage> Handler<SubscribeEvent<WebSocketEvent>> for WebSocketClient<M> {
    type Result = ();

    fn handle(&mut self, msg: SubscribeEvent<WebSocketEvent>, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(ref tx) = self.command_tx {
            tx.send(ConnectionCommand::AddEventSubscriber(msg.0))
                .map_err(|e| error!("添加订阅者失败: {}", e))
                .ok();
            info!("订阅请求已发送");
        }
    }
}

impl<M: WebSocketMessage> Handler<SubscribeEvent<M>> for WebSocketClient<M> {
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

/// WebSocket 连接管理器
/// 负责：实际的 WebSocket 连接、重连、消息收发
pub struct WebSocketConnection<C, M>
where
    C: Actor + Handler<CommandMessage>,
    C::Context: actix::dev::ToEnvelope<C, CommandMessage>,
    M: WebSocketMessage,
{
    _marker1: std::marker::PhantomData<C>,
    _marker2: std::marker::PhantomData<M>,
}

impl<C, M> WebSocketConnection<C, M>
where
    C: Actor + Handler<CommandMessage>,
    C::Context: actix::dev::ToEnvelope<C, CommandMessage>,
    M: WebSocketMessage,
{
    pub async fn run(
        url: String,
        reconnect_interval: Duration,
        proxy: Option<String>,
        mut command_rx: mpsc::UnboundedReceiver<ConnectionCommand<C, M>>,
        message_cache: Arc<Mutex<Vec<WsMessage>>>,
    ) {
        let mut event_subscribers: Vec<Recipient<WebSocketEvent>> = Vec::new();
        let mut message_subscribers: Vec<Recipient<M>> = Vec::new();
        let mut client_addr: Option<Addr<C>> = None;

        loop {
            info!("正在连接到 WebSocket: {}", url);
            Self::notify_subscribers(&event_subscribers, WebSocketEvent::Reconnecting).await;

            let initial_command = message_cache.lock().ok().map(|cache| cache.clone());

            match Self::connect_and_run(
                &url,
                &proxy,
                &mut event_subscribers,
                &mut message_subscribers,
                &mut command_rx,
                initial_command,
                &mut client_addr,
            )
            .await
            {
                Ok(_) => info!("连接正常关闭"),
                Err(e) => error!("连接错误: {}", e),
            }

            warn!("将在 {} 秒后重新连接...", reconnect_interval.as_secs());
            sleep(reconnect_interval).await;
        }
    }

    async fn connect_and_run(
        url: &str,
        proxy: &Option<String>,
        event_subscribers: &mut Vec<Recipient<WebSocketEvent>>,
        message_subscribers: &mut Vec<Recipient<M>>,
        command_rx: &mut mpsc::UnboundedReceiver<ConnectionCommand<C, M>>,
        initial_command: Option<Vec<WsMessage>>,
        client_addr: &mut Option<Addr<C>>,
    ) -> Result<(), LiError> {
        let (ws_stream, _) = if let Some(proxy_url) = proxy {
            info!("使用代理连接: {}", proxy_url);
            Self::connect_with_proxy(url, proxy_url).await?
        } else {
            info!("直接连接（无代理）");
            connect_async(url).await.map_err(|e| LiError::CustomError(format!("连接失败: {}", e)))?
        };

        info!("WebSocket 连接成功!");

        // 如果有 client_addr，则发送带地址的 Connected 事件
        if let Some(addr) = client_addr.as_ref() {
            let r: Recipient<CommandMessage> = addr.clone().recipient();
            Self::notify_subscribers(event_subscribers, WebSocketEvent::Connected(r)).await;
        } else {
            warn!("client_addr 未设置，等待地址设置后再通知");
        }

        let (mut write, mut read) = ws_stream.split();
        let (ws_tx, mut ws_rx) = mpsc::unbounded_channel::<WsMessage>();

        if let Some(commands) = initial_command {
            info!("开始发初始化消息! 数量: {}", commands.len());
            for event in commands {
                ws_tx
                    .send(event)
                    .map_err(|e| LiError::CustomError(format!("重发缓存文本消息失败: {}", e)))?;
            }
        }

        loop {
            tokio::select! {
                // 处理来自 Actor 的命令
                Some(command) = command_rx.recv() => {
                    match command {
                        ConnectionCommand::AddEventSubscriber(recipient) => {
                            event_subscribers.push(recipient.clone());
                            //添加订阅者的时候，给他发送Connected事件。
                            if let Some(addr) = client_addr.as_ref() {
                                   recipient.do_send(WebSocketEvent::Connected(addr.clone().recipient()));
                            }
                            info!("新的订阅者加入，当前订阅者数: {}", event_subscribers.len());
                        }
                        ConnectionCommand::AddMessageSubscriber(recipient) => {
                            message_subscribers.push(recipient.clone());
                            info!("新的订阅者加入，当前订阅者数: {}", message_subscribers.len());
                        }
                        ConnectionCommand::SendMessage(msg) => {
                            info!("发送消息到 WebSocket");
                            if let Err(e) = ws_tx.send(msg) {
                                error!("消息入队失败: {}", e);
                            }
                        }
                        ConnectionCommand::SetClientAddr(addr) => {
                            info!("WebSocketClient 地址已设置");
                            *client_addr = Some(addr.clone());
                            Self::notify_subscribers(event_subscribers, WebSocketEvent::Connected(addr.clone().recipient())).await;

                        }
                }
                }
                // 处理接收到的 WebSocket 消息
                message = read.next() => {
                    match message {
                        Some(Ok(msg)) => {
                            match msg {
                                WsMessage::Text(text) => {
                                    let text_str = String::from_utf8_lossy(text.as_bytes()).to_string();
                                    trace!("收到文本消息");
                                    match M::from_text(&text_str) {
                                        Ok(m) => {
                                            Self::notify_message_subscribers(message_subscribers, m).await;
                                        },
                                        Err(e) => {
                                            error!("解析文本消息失败: {}", e);
                                            continue;
                                        }
                                    };
                                }
                                WsMessage::Binary(data) => {
                                    let data_vec = data.to_vec();
                                    trace!("收到二进制消息: {} 字节", data_vec.len());
                                    match M::from_binary(data_vec) {
                                        Ok(m) => {
                                            Self::notify_message_subscribers(message_subscribers, m).await;
                                        },
                                        Err(e) => {
                                            error!("解析文本消息失败: {}", e);
                                            continue;
                                        }
                                    };
                                }
                                WsMessage::Ping(data) => {
                                    trace!("收到 Ping");
                                    if let Err(e) = write.send(WsMessage::Pong(data)).await {
                                        error!("发送 Pong 失败: {}", e);
                                        return Err(LiError::CustomError(format!("发送 Pong 失败: {}", e)));
                                    }
                                }
                                WsMessage::Pong(_) => {
                                    trace!("收到 Pong");
                                }
                                WsMessage::Close(frame) => {
                                    trace!("收到关闭帧: {:?}", frame);
                                    Self::notify_subscribers(event_subscribers, WebSocketEvent::Disconnected).await;
                                    return Ok(());
                                }
                                WsMessage::Frame(_) => {}
                            }
                        }
                        Some(Err(e)) => {
                            error!("接收消息错误: {}", e);
                            Self::notify_subscribers(event_subscribers, WebSocketEvent::Error(e.to_string())).await;
                            return Err(LiError::CustomError(format!("接收消息错误: {}", e)));
                        }
                        None => {
                            warn!("WebSocket 流已关闭");
                            Self::notify_subscribers(event_subscribers, WebSocketEvent::Disconnected).await;
                            return Ok(());
                        }
                    }
                }
                // 处理要发送的 WebSocket 消息
                Some(msg) = ws_rx.recv() => {
                    info!("实际发送消息,{}",msg);
                    if let Err(e) = write.send(msg).await {
                        error!("发送消息失败: {}", e);
                        return Err(LiError::CustomError(format!("发送消息失败: {}", e)));
                    }
                }
                // 心跳
                _ = sleep(Duration::from_secs(60)) => {
                    let ping = WsMessage::Ping(vec![1u8, 2u8, 3u8].into());
                    if let Err(e) = write.send(ping).await {
                        error!("发送心跳 Ping 失败: {}", e);
                        return Err(LiError::CustomError(format!("发送心跳 Ping 失败: {}", e)));
                    }
                    debug!("Sent Ping");
                }
            }
        }
    }

    async fn notify_message_subscribers(subscribers: &[Recipient<M>], event: M) {
        for subscriber in subscribers.iter() {
            subscriber.do_send(event.clone());
        }
    }

    async fn notify_subscribers(subscribers: &[Recipient<WebSocketEvent>], event: WebSocketEvent) {
        for subscriber in subscribers.iter() {
            subscriber.do_send(event.clone());
        }
    }

    /// 通过代理连接 WebSocket
    async fn connect_with_proxy(
        url: &str,
        proxy_url: &str,
    ) -> Result<
        (
            tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
            tokio_tungstenite::tungstenite::handshake::client::Response,
        ),
        LiError,
    > {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // 解析代理地址
        let proxy_uri: Uri = proxy_url.parse().map_err(|e| LiError::CustomError(format!("代理地址解析失败: {}", e)))?;

        // 解析目标 WebSocket 地址
        let ws_uri: Uri = url.parse().map_err(|e| LiError::CustomError(format!("WebSocket地址解析失败: {}", e)))?;

        // 获取代理主机和端口
        let proxy_host = proxy_uri.host().ok_or_else(|| LiError::CustomError("代理地址缺少主机名".to_string()))?;
        let proxy_port = proxy_uri.port_u16().unwrap_or(8080);

        // 获取目标主机和端口
        let target_host = ws_uri.host().ok_or_else(|| LiError::CustomError("WebSocket地址缺少主机名".to_string()))?;
        let target_port = ws_uri.port_u16().unwrap_or(if ws_uri.scheme_str() == Some("wss") { 443 } else { 80 });

        // 连接到代理服务器
        let proxy_addr = format!("{}:{}", proxy_host, proxy_port);
        info!("连接到代理服务器: {}", proxy_addr);
        let mut stream = tokio::net::TcpStream::connect(&proxy_addr)
            .await
            .map_err(|e| LiError::CustomError(format!("连接代理服务器失败 {}: {}", proxy_addr, e)))?;

        // 发送 HTTP CONNECT 请求
        let connect_request = format!(
            "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n\r\n",
            target_host, target_port, target_host, target_port
        );

        stream
            .write_all(connect_request.as_bytes())
            .await
            .map_err(|e| LiError::CustomError(format!("发送CONNECT请求失败: {}", e)))?;

        // 读取代理响应
        let mut buffer = vec![0u8; 1024];
        let n = stream
            .read(&mut buffer)
            .await
            .map_err(|e| LiError::CustomError(format!("读取代理响应失败: {}", e)))?;

        let response = String::from_utf8_lossy(&buffer[..n]);
        if !response.starts_with("HTTP/1.1 200") && !response.starts_with("HTTP/1.0 200") {
            return Err(LiError::CustomError(format!(
                "代理连接失败: {}",
                response.lines().next().unwrap_or("未知错误")
            )));
        }

        info!("代理隧道建立成功");

        // 通过代理隧道建立 WebSocket 连接
        let request = url
            .to_string()
            .into_client_request()
            .map_err(|e| LiError::CustomError(format!("创建WebSocket请求失败: {}", e)))?;

        // 如果是 WSS，需要 TLS 包装
        if ws_uri.scheme_str() == Some("wss") {
            use tokio_tungstenite::Connector;
            let connector = Connector::NativeTls(
                tokio_native_tls::native_tls::TlsConnector::new().map_err(|e| LiError::CustomError(format!("创建TLS连接器失败: {}", e)))?,
            );

            tokio_tungstenite::client_async_tls_with_config(request, stream, None, Some(connector))
                .await
                .map_err(|e| LiError::CustomError(format!("通过代理连接WebSocket失败: {}", e)))
        } else {
            // 对于非TLS连接，需要手动包装为 MaybeTlsStream
            use tokio_tungstenite::MaybeTlsStream;
            let tls_stream = MaybeTlsStream::Plain(stream);
            tokio_tungstenite::client_async(request, tls_stream)
                .await
                .map_err(|e| LiError::CustomError(format!("通过代理连接WebSocket失败: {}", e)))
        }
    }
}
