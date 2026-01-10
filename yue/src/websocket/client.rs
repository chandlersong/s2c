use crate::errors::YueError;
use actix::Message as ActixMessage;
use actix::{Actor, Context, Handler, Recipient};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

/// WebSocket 事件，发送给订阅者
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WebSocketEvent {
    /// 连接成功
    Connected,
    /// 连接断开
    Disconnected,
    /// 收到文本消息
    TextMessage(String),
    /// 收到二进制消息
    BinaryMessage(Vec<u8>),
    /// 重连中
    Reconnecting,
    /// 错误
    Error(String),
}

impl ActixMessage for WebSocketEvent {
    type Result = ();
}

/// 发送文本消息
#[derive(Clone, Debug)]
pub struct SendTextMessage {
    pub text: String,
}

impl ActixMessage for SendTextMessage {
    type Result = Result<(), YueError>;
}

/// 发送二进制消息
#[derive(Clone, Debug)]
pub struct SendBinaryMessage {
    pub data: Vec<u8>,
}

impl ActixMessage for SendBinaryMessage {
    type Result = Result<(), YueError>;
}

/// 订阅 WebSocket 事件
pub struct SubscribeToEvents {
    pub recipient: Recipient<WebSocketEvent>,
}

impl ActixMessage for SubscribeToEvents {
    type Result = Result<(), YueError>;
}

/// 内部命令，用于 Actor 和 Connection 之间通信
enum InternalCommand {
    /// 添加订阅者
    AddSubscriber(Recipient<WebSocketEvent>),
    /// 发送 WebSocket 消息
    SendMessage(WsMessage),
}

/// WebSocket 客户端 Actor
/// 负责：订阅管理、消息发送的外部接口
pub struct WebSocketClient {
    url: String,
    reconnect_interval: Duration,
    proxy: Option<String>,
    /// 向内部连接发送命令
    command_tx: Option<mpsc::UnboundedSender<InternalCommand>>,
    /// 消息缓存
    /// 这个缓存，放在connection里面可能更加好一点。但是放在client里面，主要是为了以后的更改和去除。
    command_cache: Arc<Mutex<Vec<WsMessage>>>,
    /// 重连时是否重新发送缓存的消息,首先默认重新发送
    resend_command_on_reconnect: bool,
}

impl WebSocketClient {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            reconnect_interval: Duration::from_secs(5),
            proxy: None,
            command_tx: None,
            command_cache: Arc::new(Mutex::new(Vec::new())),
            resend_command_on_reconnect: true,
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

    /// 设置重连时是否重新发送缓存消息
    pub fn with_resend_cached_on_reconnect(mut self, resend: bool) -> Self {
        self.resend_command_on_reconnect = resend;
        self
    }

    /// 添加消息到缓存
    pub fn add_to_cache(&self, event: WsMessage) {
        if !self.resend_command_on_reconnect {
            return;
        }

        let mut cache = self.command_cache.lock().unwrap();
        cache.push(event);
    }

    /// 清空缓存
    pub fn clear_command_cache(&self) {
        self.command_cache.lock().unwrap().clear();
    }

    /// 检查是否应该在重连时重新发送缓存消息
    pub fn should_resend_command_on_reconnect(&self) -> bool {
        self.resend_command_on_reconnect
    }

    #[cfg(test)]
    fn cache_len(&self) -> usize {
        self.command_cache.lock().unwrap().len()
    }

    #[cfg(test)]
    fn cached_events(&self) -> Vec<WsMessage> {
        self.command_cache.lock().unwrap().clone()
    }
}

impl Actor for WebSocketClient {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        let url = self.url.clone();
        let reconnect_interval = self.reconnect_interval;
        let proxy = self.proxy.clone();

        let resend_cached_on_reconnect = self.resend_command_on_reconnect;

        // 创建命令通道
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        self.command_tx = Some(command_tx);
        info!(
            "WebSocketClient started, connecting to {},send command at initial: {}",
            url, resend_cached_on_reconnect
        );

        let message_cache = if self.resend_command_on_reconnect {
            Some(self.command_cache.clone())
        } else {
            None
        };

        // 启动内部连接管理
        tokio::spawn(async move {
            WebSocketConnection::run(url, reconnect_interval, proxy, command_rx, message_cache).await;
        });
    }
}

impl Handler<SendTextMessage> for WebSocketClient {
    type Result = Result<(), YueError>;

    fn handle(&mut self, msg: SendTextMessage, _ctx: &mut Context<Self>) -> Self::Result {
        if let Some(ref tx) = self.command_tx {
            let message = WsMessage::Text(msg.text.into());
            self.add_to_cache(message.clone());
            tx.send(InternalCommand::SendMessage(message))
                .map_err(|e| YueError::CustomError(format!("发送文本消息失败: {}", e)))
        } else {
            Err(YueError::CustomError("WebSocket 客户端未初始化".to_string()))
        }
    }
}

impl Handler<SendBinaryMessage> for WebSocketClient {
    type Result = Result<(), YueError>;

    fn handle(&mut self, msg: SendBinaryMessage, _ctx: &mut Context<Self>) -> Self::Result {
        if let Some(ref tx) = self.command_tx {
            let message = WsMessage::Binary(msg.data.into());
            self.add_to_cache(message.clone());
            tx.send(InternalCommand::SendMessage(message))
                .map_err(|e| YueError::CustomError(format!("发送二进制消息失败: {}", e)))
        } else {
            Err(YueError::CustomError("WebSocket 客户端未初始化".to_string()))
        }
    }
}

impl Handler<SubscribeToEvents> for WebSocketClient {
    type Result = Result<(), YueError>;

    fn handle(&mut self, msg: SubscribeToEvents, _ctx: &mut Context<Self>) -> Self::Result {
        if let Some(ref tx) = self.command_tx {
            tx.send(InternalCommand::AddSubscriber(msg.recipient))
                .map_err(|e| YueError::CustomError(format!("添加订阅者失败: {}", e)))?;
            info!("订阅请求已发送");
            Ok(())
        } else {
            Err(YueError::CustomError("WebSocket 客户端未初始化".to_string()))
        }
    }
}

/// WebSocket 连接管理器
/// 负责：实际的 WebSocket 连接、重连、消息收发
struct WebSocketConnection;

impl WebSocketConnection {
    async fn run(
        url: String,
        reconnect_interval: Duration,
        proxy: Option<String>,
        mut command_rx: mpsc::UnboundedReceiver<InternalCommand>,
        message_cache: Option<Arc<Mutex<Vec<WsMessage>>>>,
    ) {
        let mut subscribers: Vec<Recipient<WebSocketEvent>> = Vec::new();

        loop {
            info!("正在连接到 WebSocket: {}", url);
            Self::notify_subscribers(&subscribers, WebSocketEvent::Reconnecting).await;
            let initial_command = if let Some(cache) = &message_cache {
                let cached_commands = cache.lock().unwrap().clone();
                Some(cached_commands)
            } else {
                None
            };
            match Self::connect_and_run(&url, &proxy, &mut subscribers, &mut command_rx, initial_command).await {
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
        subscribers: &mut Vec<Recipient<WebSocketEvent>>,
        command_rx: &mut mpsc::UnboundedReceiver<InternalCommand>,
        initial_command: Option<Vec<WsMessage>>,
    ) -> Result<(), YueError> {
        let (ws_stream, _) = if let Some(proxy_url) = proxy {
            info!("使用代理连接: {}", proxy_url);
            Self::connect_with_proxy(url, proxy_url).await?
        } else {
            info!("直接连接（无代理）");
            connect_async(url).await.map_err(|e| YueError::CustomError(format!("连接失败: {}", e)))?
        };

        info!("WebSocket 连接成功!");
        Self::notify_subscribers(subscribers, WebSocketEvent::Connected).await;

        let (mut write, mut read) = ws_stream.split();
        let (ws_tx, mut ws_rx) = mpsc::unbounded_channel::<WsMessage>();

        if let Some(commands) = initial_command {
            for event in commands {
                ws_tx
                    .send(event)
                    .map_err(|e| YueError::CustomError(format!("重发缓存文本消息失败: {}", e)))?;
            }
        }

        loop {
            tokio::select! {
                // 处理来自 Actor 的命令
                Some(command) = command_rx.recv() => {
                    match command {
                        InternalCommand::AddSubscriber(recipient) => {
                            subscribers.push(recipient);
                            info!("新的订阅者加入，当前订阅者数: {}", subscribers.len());
                        }
                        InternalCommand::SendMessage(msg) => {
                            info!("发送消息到 WebSocket");
                            if let Err(e) = ws_tx.send(msg) {
                                error!("消息入队失败: {}", e);
                            }
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
                                    Self::notify_subscribers(subscribers, WebSocketEvent::TextMessage(text_str)).await;
                                }
                                WsMessage::Binary(data) => {
                                    let data_vec = data.to_vec();
                                    trace!("收到二进制消息: {} 字节", data_vec.len());
                                    Self::notify_subscribers(subscribers, WebSocketEvent::BinaryMessage(data_vec)).await;
                                }
                                WsMessage::Ping(data) => {
                                    trace!("收到 Ping");
                                    if let Err(e) = write.send(WsMessage::Pong(data)).await {
                                        error!("发送 Pong 失败: {}", e);
                                        return Err(YueError::CustomError(format!("发送 Pong 失败: {}", e)));
                                    }
                                }
                                WsMessage::Pong(_) => {
                                    trace!("收到 Pong");
                                }
                                WsMessage::Close(frame) => {
                                    trace!("收到关闭帧: {:?}", frame);
                                    Self::notify_subscribers(subscribers, WebSocketEvent::Disconnected).await;
                                    return Ok(());
                                }
                                WsMessage::Frame(_) => {}
                            }
                        }
                        Some(Err(e)) => {
                            error!("接收消息错误: {}", e);
                            Self::notify_subscribers(subscribers, WebSocketEvent::Error(e.to_string())).await;
                            return Err(YueError::CustomError(format!("接收消息错误: {}", e)));
                        }
                        None => {
                            warn!("WebSocket 流已关闭");
                            Self::notify_subscribers(subscribers, WebSocketEvent::Disconnected).await;
                            return Ok(());
                        }
                    }
                }
                // 处理要发送的 WebSocket 消息
                Some(msg) = ws_rx.recv() => {
                    info!("实际发送消息");
                    if let Err(e) = write.send(msg).await {
                        error!("发送消息失败: {}", e);
                        return Err(YueError::CustomError(format!("发送消息失败: {}", e)));
                    }
                }
                // 心跳
                _ = sleep(Duration::from_secs(60)) => {
                    let ping = WsMessage::Ping(vec![1u8, 2u8, 3u8].into());
                    if let Err(e) = write.send(ping).await {
                        error!("发送心跳 Ping 失败: {}", e);
                        return Err(YueError::CustomError(format!("发送心跳 Ping 失败: {}", e)));
                    }
                    debug!("Sent Ping");
                }
            }
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
        YueError,
    > {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // 解析代理地址
        let proxy_uri: Uri = proxy_url.parse().map_err(|e| YueError::CustomError(format!("代理地址解析失败: {}", e)))?;

        // 解析目标 WebSocket 地址
        let ws_uri: Uri = url.parse().map_err(|e| YueError::CustomError(format!("WebSocket地址解析失败: {}", e)))?;

        // 获取代理主机和端口
        let proxy_host = proxy_uri.host().ok_or_else(|| YueError::CustomError("代理地址缺少主机名".to_string()))?;
        let proxy_port = proxy_uri.port_u16().unwrap_or(8080);

        // 获取目标主机和端口
        let target_host = ws_uri
            .host()
            .ok_or_else(|| YueError::CustomError("WebSocket地址缺少主机名".to_string()))?;
        let target_port = ws_uri.port_u16().unwrap_or(if ws_uri.scheme_str() == Some("wss") { 443 } else { 80 });

        // 连接到代理服务器
        let proxy_addr = format!("{}:{}", proxy_host, proxy_port);
        info!("连接到代理服务器: {}", proxy_addr);
        let mut stream = tokio::net::TcpStream::connect(&proxy_addr)
            .await
            .map_err(|e| YueError::CustomError(format!("连接代理服务器失败 {}: {}", proxy_addr, e)))?;

        // 发送 HTTP CONNECT 请求
        let connect_request = format!(
            "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n\r\n",
            target_host, target_port, target_host, target_port
        );

        stream
            .write_all(connect_request.as_bytes())
            .await
            .map_err(|e| YueError::CustomError(format!("发送CONNECT请求失败: {}", e)))?;

        // 读取代理响应
        let mut buffer = vec![0u8; 1024];
        let n = stream
            .read(&mut buffer)
            .await
            .map_err(|e| YueError::CustomError(format!("读取代理响应失败: {}", e)))?;

        let response = String::from_utf8_lossy(&buffer[..n]);
        if !response.starts_with("HTTP/1.1 200") && !response.starts_with("HTTP/1.0 200") {
            return Err(YueError::CustomError(format!(
                "代理连接失败: {}",
                response.lines().next().unwrap_or("未知错误")
            )));
        }

        info!("代理隧道建立成功");

        // 通过代理隧道建立 WebSocket 连接
        let request = url
            .to_string()
            .into_client_request()
            .map_err(|e| YueError::CustomError(format!("创建WebSocket请求失败: {}", e)))?;

        // 如果是 WSS，需要 TLS 包装
        if ws_uri.scheme_str() == Some("wss") {
            use tokio_tungstenite::Connector;
            let connector = Connector::NativeTls(
                tokio_native_tls::native_tls::TlsConnector::new().map_err(|e| YueError::CustomError(format!("创建TLS连接器失败: {}", e)))?,
            );

            tokio_tungstenite::client_async_tls_with_config(request, stream, None, Some(connector))
                .await
                .map_err(|e| YueError::CustomError(format!("通过代理连接WebSocket失败: {}", e)))
        } else {
            // 对于非TLS连接，需要手动包装为 MaybeTlsStream
            use tokio_tungstenite::MaybeTlsStream;
            let tls_stream = MaybeTlsStream::Plain(stream);
            tokio_tungstenite::client_async(request, tls_stream)
                .await
                .map_err(|e| YueError::CustomError(format!("通过代理连接WebSocket失败: {}", e)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_enabled_by_default() {
        let client = WebSocketClient::new("ws://example.com");
        assert!(client.should_resend_command_on_reconnect());
        assert_eq!(client.cache_len(), 0);

        client.add_to_cache(WsMessage::Text("hello".into()));
        assert_eq!(client.cache_len(), 1);
    }

    #[test]
    fn disable_cache_stops_storing() {
        let client = WebSocketClient::new("ws://example.com").with_resend_cached_on_reconnect(false);
        assert!(!client.should_resend_command_on_reconnect());

        client.add_to_cache(WsMessage::Text("ignored".into()));
        assert_eq!(client.cache_len(), 0);
    }

    #[test]
    fn clear_cache_works() {
        let client = WebSocketClient::new("ws://example.com");
        client.add_to_cache(WsMessage::Text("a".into()));
        client.add_to_cache(WsMessage::binary((vec![1, 2, 3])));
        assert_eq!(client.cache_len(), 2);

        client.clear_command_cache();
        assert_eq!(client.cache_len(), 0);
    }
}
