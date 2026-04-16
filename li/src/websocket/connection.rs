use crate::errors::LiError;
use crate::websocket::models::WebSocketMessage;
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, trace, warn};
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{broadcast, mpsc};
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

/// WebSocket 事件，发送给订阅者
#[derive(Clone, Debug)]
pub enum WebSocketEvent {
    /// 连接成功，携带 WebSocketClient 的地址
    Connected(UnboundedSender<CommandMessage>),
    /// 连接断开
    Disconnected,
    /// 重连中
    Reconnecting,
    /// 错误
    Error(String),
}

pub enum CommandMessage {
    Connection(ConnectionAction),
    ToServer(ToServerMessage),
}

pub enum ConnectionAction {
    Close,
    Reconnection,
}

///
/// 客户端给服务器端发送的消息。
/// Binary暂时先不管，也就是一个new的区别
///
#[derive(Clone, Debug)]
pub enum ToServerMessage {
    Text(String, bool),
    Binary(Vec<u8>, bool),
}

impl ToServerMessage {
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
            ToServerMessage::Text(txt, resend) => (WsMessage::Text(txt.clone().into()), *resend),
            ToServerMessage::Binary(data, resend) => (WsMessage::Binary(data.clone().into()), resend.clone()),
        }
    }
}

pub const MESSAGE_CACHE: usize = 1000;
const EVENT_CACHE: usize = 100;

#[derive(Clone)]
pub struct WebSocketInterface<M>
where
    M: WebSocketMessage,
{
    message_broadcast: Option<broadcast::Sender<M>>,
    event_broadcast: broadcast::Sender<WebSocketEvent>,
    command_sender: UnboundedSender<CommandMessage>,
}

impl<M> WebSocketInterface<M>
where
    M: WebSocketMessage,
{
    pub fn new(
        message_broadcast: Option<broadcast::Sender<M>>,
        event_broadcast: broadcast::Sender<WebSocketEvent>,
        command_sender: UnboundedSender<CommandMessage>,
    ) -> Self {
        Self {
            message_broadcast,
            event_broadcast,
            command_sender,
        }
    }

    pub fn get_message_receiver(&self) -> Option<broadcast::Receiver<M>> {
        if let Some(broadcast) = &self.message_broadcast {
            Some(broadcast.subscribe())
        } else {
            None
        }
    }

    pub fn get_event_broadcast(&self) -> broadcast::Receiver<WebSocketEvent> {
        self.event_broadcast.subscribe()
    }

    pub fn command_sender(&self) -> UnboundedSender<CommandMessage> {
        self.command_sender.clone()
    }
}

pub type MessageHandler<M> = Box<dyn MessageHandlerTrait<M> + Send + Sync>;
pub type ShareMessageHandler<M> = Arc<dyn MessageHandlerTrait<M> + Send + Sync>;
///
/// 主要是对message做点定制化操纵的handler。比如分发消息
///
#[async_trait]
pub trait MessageHandlerTrait<M: WebSocketMessage> {
    /// 处理收到的消息。
    async fn handle_message(&self, message: &M);

    /// 提供Sender方便其他人处理
    /// 并不是每个下流都要服务都要监听。只要写个handler处理一下就好了。
    /// 所以这里用这种方式。
    fn get_tx(&self) -> Option<broadcast::Sender<M>> {
        None
    }
}

pub struct BroadcastMessageHandler<M: WebSocketMessage> {
    message_broadcast: broadcast::Sender<M>,
}

impl<M: WebSocketMessage> BroadcastMessageHandler<M> {
    pub fn new() -> ShareMessageHandler<M> {
        let (message_broadcast, _) = broadcast::channel(MESSAGE_CACHE);
        Arc::new(Self { message_broadcast })
    }
}
#[async_trait]
impl<M: WebSocketMessage> MessageHandlerTrait<M> for BroadcastMessageHandler<M> {
    async fn handle_message(&self, message: &M) {
        if self.message_broadcast.receiver_count() == 0 {
            trace!("没有订阅者，消息将被丢弃");
            return;
        }
        if let Err(e) = self.message_broadcast.send(message.clone()) {
            // 这个感觉会很多，所以就debug了
            error!("广播消息失败: {}", e);
        }
    }

    fn get_tx(&self) -> Option<broadcast::Sender<M>> {
        Some(self.message_broadcast.clone())
    }
}

/// WebSocket 连接管理器
/// 负责：实际的 WebSocket 连接、重连、消息收发
///
/// 外界和他的沟通主要是是3个方面
/// 1. 服务器发过来的消息。比如服务器端返回的k线信息
/// 2. connection状态变化，比如说重连，服务启动。
/// 3. 客户端给服务器发送消息。比如说订阅命令。
///
/// 所以这个启动的返回值，就是这个
///
pub struct WebSocketConnection {}

impl WebSocketConnection {
    /// 启动 WebSocket 连接管理器。
    ///
    /// 如果 `message_handler` 为 Some(...) 则使用用户提供的处理器；否则使用
    /// 默认的 `BroadcastMessageHandler`，它会把消息广播到 `message_tx`。
    pub async fn run<M>(
        url: String,
        reconnect_interval: Duration,
        proxy: Option<String>,
        message_handler: Option<ShareMessageHandler<M>>,
    ) -> Arc<WebSocketInterface<M>>
    where
        M: WebSocketMessage + Send + Sync + 'static,
    {
        //FUTURE: 这些channel的宽度，通过参数传进来，现在写的话，感觉参数太多。
        let m_handler = message_handler.unwrap_or_else(|| BroadcastMessageHandler::new());
        let message_tx = m_handler.get_tx();
        let (event_tx, _) = broadcast::channel(EVENT_CACHE);
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let res = Arc::new(WebSocketInterface::new(message_tx.clone(), event_tx.clone(), command_tx.clone()));

        // 准备 handler：如果用户没有提供，则构造默认的 BroadcastMessageHandler

        tokio::spawn(async move {
            loop {
                info!("正在连接到 WebSocket: {}", url);
                if let Err(e) = event_tx.send(WebSocketEvent::Reconnecting) {
                    error!("sending {}", e);
                }
                let mut message_cache = vec![];

                match Self::connect_and_run(
                    &url,
                    &proxy,
                    &event_tx,
                    &command_tx,
                    &mut command_rx,
                    &mut message_cache,
                    m_handler.clone(),
                )
                .await
                {
                    Ok(action) => match action {
                        ConnectionAction::Reconnection => {
                            info!("连接重启：{}", url);
                        }
                        ConnectionAction::Close => {
                            info!("连接关闭：{}", url);
                            break;
                        }
                    },
                    Err(e) => error!("连接错误: {}", e),
                }

                warn!("将在 {} 秒后重新连接...", reconnect_interval.as_secs());
                sleep(reconnect_interval).await;
            }
        });
        res
    }

    async fn connect_and_run<M: WebSocketMessage>(
        url: &str,
        proxy: &Option<String>,
        event_tx: &broadcast::Sender<WebSocketEvent>,
        command_tx: &UnboundedSender<CommandMessage>,
        command_rx: &mut UnboundedReceiver<CommandMessage>,
        initial_command: &mut Vec<WsMessage>,
        message_handler: ShareMessageHandler<M>,
    ) -> Result<ConnectionAction, LiError> {
        let (ws_stream, _) = if let Some(proxy_url) = proxy {
            info!("使用代理连接: {}", proxy_url);
            Self::connect_with_proxy(url, proxy_url).await?
        } else {
            info!("直接连接（无代理）");
            connect_async(url).await.map_err(|e| LiError::CustomError(format!("连接失败: {}", e)))?
        };

        info!("WebSocket 连接成功!");

        if let Err(e) = event_tx.send(WebSocketEvent::Connected(command_tx.clone().into())) {
            error!("sending websocket started event:{}", e);
        }

        let (mut write, mut read) = ws_stream.split();
        let (ws_tx, mut ws_rx) = mpsc::unbounded_channel::<WsMessage>();

        info!("开始发初始化消息! 数量: {}", initial_command.len());
        // 使用 iter() 避免移动 initial_command（后面需要继续使用它）
        for event in initial_command.iter() {
            if let Err(e) = ws_tx.send(event.clone()) {
                error!("error sending websocket event at first connection:{}", e);
            }
        }

        loop {
            tokio::select! {
                // 处理来自 Actor 的命令
                Some(command) = command_rx.recv() => {
                    // 注意：`ws_tx.send` 会取得消息的所有权，如果先 send 再使用
                    // 原始变量会导致 "use of moved value" 编译错误。因此先
                    // 使用 clone() 发出去，保留原始 msg 以便在需要时缓存。
                    match command {
                        CommandMessage::Connection(action) => {
                            let close_frame = Some(CloseFrame {
                                code: CloseCode::Normal,           // 1000: 正常关闭
                                reason: "See you soon".into(),
                            });

                            if let Err(e) = write.send(WsMessage::Close(close_frame)).await {
                                error!("Failed to send close frame: {},url:{}", e, url);
                            }
                               // 可选：显式关闭 sink（有助于 flush 并关闭）
                            if let Err(e) = write.close().await {
                                error!("调用 write.close() 失败: {},url:{}", e, url);
                            } else {
                                debug!("connection关闭:{}",url);
                            }
                            return Ok(action);
                        }
                        CommandMessage::ToServer(to_server_message) => {
                            let (msg, resend) = to_server_message.to_ws_message();
                            if let Err(e) = ws_tx.send(msg.clone()) {
                                error!("消息入队失败: {}", e);
                            }
                            if resend {
                                initial_command.push(msg);
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
                                    match M::from_text(&text_str) {
                                        Ok(m) => {
                                            // 交给 message_handler 处理（可能是广播、也可能是用户自定义处理）
                                            message_handler.handle_message(&m).await;
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
                                            // 交给 message_handler 处理（可能是广播、也可能是用户自定义处理）
                                            message_handler.handle_message(&m).await;
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
                                    if let Err(e) =event_tx.send(WebSocketEvent::Disconnected){
                                        // 感觉这个会很多，所以也就这样处理了。
                                        debug!("send error {}", e);
                                    };
                                    return Ok(ConnectionAction::Reconnection);
                                }
                                WsMessage::Frame(_) => {}
                            }
                        }
                        Some(Err(e)) => {
                            error!("接收消息错误: {}", e);
                            if let Err(e) =event_tx.send(WebSocketEvent::Error(e.to_string())){
                                    // 感觉这个会很多，所以也就这样处理了。
                                    debug!("send error {}", e);
                            };
                            return Err(LiError::CustomError(format!("接收消息错误: {}", e)));
                        }
                        None => {
                            warn!("WebSocket 流已关闭");
                            if let Err(e) =event_tx.send(WebSocketEvent::Disconnected){
                                    // 感觉这个会很多，所以也就这样处理了。
                                    debug!("send error {}", e);
                            };
                            return Ok(ConnectionAction::Reconnection);
                        }
                    }
                }
                // 处理要发送的 WebSocket 消息
                Some(msg) = ws_rx.recv() => {
                    debug!("实际发送消息,{}",msg);
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
