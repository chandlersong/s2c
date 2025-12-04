use crate::errors::YueError;
use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use std::time::Duration;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// WebSocket 客户端
///
/// 功能：
/// 1. 简洁易懂的API
/// 2. 自动重连机制
/// 3. 打印所有接收到的消息
/// 4. 代理支持
pub struct WebSocketClient {
    url: String,
    reconnect_interval: Duration,
    proxy: Option<String>,
}

impl WebSocketClient {
    /// 创建新的 WebSocket 客户端
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            reconnect_interval: Duration::from_secs(5),
            proxy: None,
        }
    }

    /// 创建新的 WebSocket 客户端，从环境变量读取代理设置
    ///
    /// 支持的环境变量：HTTP_PROXY, http_proxy, HTTPS_PROXY, https_proxy
    pub fn new_with_env_proxy(url: impl Into<String>) -> Self {
        let proxy = std::env::var("HTTP_PROXY")
            .or_else(|_| std::env::var("http_proxy"))
            .or_else(|_| std::env::var("HTTPS_PROXY"))
            .or_else(|_| std::env::var("https_proxy"))
            .ok();

        if let Some(ref p) = proxy {
            info!("使用代理: {}", p);
        }

        Self {
            url: url.into(),
            reconnect_interval: Duration::from_secs(5),
            proxy,
        }
    }

    /// 设置代理服务器
    ///
    /// # 参数
    /// * `proxy` - 代理服务器地址，例如: "http://127.0.0.1:7890"
    pub fn with_proxy(mut self, proxy: impl Into<String>) -> Self {
        self.proxy = Some(proxy.into());
        self
    }

    /// 设置重连间隔
    pub fn with_reconnect_interval(mut self, interval: Duration) -> Self {
        self.reconnect_interval = interval;
        self
    }

    /// 通过代理连接 WebSocket
    /// 使用 HTTP CONNECT 方法建立代理隧道
    async fn connect_with_proxy(
        &self,
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
        let ws_uri: Uri = self
            .url
            .parse()
            .map_err(|e| YueError::CustomError(format!("WebSocket地址解析失败: {}", e)))?;

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
        let request = self
            .url
            .clone()
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

    /// 连接并持续运行
    ///
    /// 自动处理断线重连，打印所有接收到的消息
    pub async fn connect_and_run(&self) -> Result<(), YueError> {
        loop {
            match self.try_connect().await {
                Ok(_) => {
                    info!("WebSocket连接正常关闭");
                }
                Err(e) => {
                    error!("WebSocket连接错误: {}", e);
                }
            }

            warn!("将在 {} 秒后重新连接...", self.reconnect_interval.as_secs());
            sleep(self.reconnect_interval).await;
        }
    }

    /// 尝试建立连接并处理消息
    async fn try_connect(&self) -> Result<(), YueError> {
        info!("正在连接到 WebSocket: {}", self.url);

        let (ws_stream, _) = if let Some(ref proxy_url) = self.proxy {
            info!("使用代理连接: {}", proxy_url);
            self.connect_with_proxy(proxy_url).await?
        } else {
            info!("直接连接（无代理）");
            connect_async(&self.url)
                .await
                .map_err(|e| YueError::CustomError(format!("连接失败: {}", e)))?
        };

        info!("WebSocket 连接成功!");

        let (mut write, mut read) = ws_stream.split();

        // 处理接收到的消息
        while let Some(message) = read.next().await {
            match message {
                Ok(msg) => {
                    match msg {
                        Message::Text(text) => {
                            info!("收到文本消息: {}", text);
                        }
                        Message::Binary(data) => {
                            info!("收到二进制消息: {} 字节", data.len());
                        }
                        Message::Ping(data) => {
                            info!("收到 Ping");
                            // 自动回复 Pong
                            if let Err(e) = write.send(Message::Pong(data)).await {
                                error!("发送 Pong 失败: {}", e);
                                return Err(YueError::CustomError(format!("发送 Pong 失败: {}", e)));
                            }
                        }
                        Message::Pong(_) => {
                            info!("收到 Pong");
                        }
                        Message::Close(frame) => {
                            info!("收到关闭帧: {:?}", frame);
                            return Ok(());
                        }
                        Message::Frame(_) => {
                            // 原始帧，通常不需要处理
                        }
                    }
                }
                Err(e) => {
                    error!("接收消息错误: {}", e);
                    return Err(YueError::CustomError(format!("接收消息错���: {}", e)));
                }
            }
        }

        Ok(())
    }

    /// 连接并发送订阅消息
    ///
    /// 适用于需要发送订阅请求的场景（如币安、OKX等交易所）
    pub async fn connect_and_subscribe(&self, subscribe_message: String) -> Result<(), YueError> {
        loop {
            match self.try_connect_and_subscribe(&subscribe_message).await {
                Ok(_) => {
                    info!("WebSocket连接正常关闭");
                }
                Err(e) => {
                    error!("WebSocket连接错误: {}", e);
                }
            }

            warn!("将在 {} 秒后重新连接...", self.reconnect_interval.as_secs());
            sleep(self.reconnect_interval).await;
        }
    }

    /// 尝试连接并发送订阅消息
    async fn try_connect_and_subscribe(&self, subscribe_message: &str) -> Result<(), YueError> {
        info!("正在连接到 WebSocket: {}", self.url);

        let (ws_stream, _) = if let Some(ref proxy_url) = self.proxy {
            info!("使用代理连接: {}", proxy_url);
            self.connect_with_proxy(proxy_url).await?
        } else {
            info!("直接连接（无代理）");
            connect_async(&self.url)
                .await
                .map_err(|e| YueError::CustomError(format!("连接失败: {}", e)))?
        };

        info!("WebSocket 连接成功!");

        let (mut write, mut read) = ws_stream.split();

        // 发送订阅消息
        info!("发送订阅消息: {}", subscribe_message);
        write
            .send(Message::Text(subscribe_message.into()))
            .await
            .map_err(|e| YueError::CustomError(format!("发送订阅消息失败: {}", e)))?;

        // 处理接收到的消息
        while let Some(message) = read.next().await {
            match message {
                Ok(msg) => {
                    match msg {
                        Message::Text(text) => {
                            info!("收到文本消息: {}", text);
                        }
                        Message::Binary(data) => {
                            info!("收到二进制消息: {} 字节", data.len());
                        }
                        Message::Ping(data) => {
                            info!("收到 Ping");
                            if let Err(e) = write.send(Message::Pong(data)).await {
                                error!("发送 Pong 失败: {}", e);
                                return Err(YueError::CustomError(format!("发送 Pong 失败: {}", e)));
                            }
                        }
                        Message::Pong(_) => {
                            info!("收到 Pong");
                        }
                        Message::Close(frame) => {
                            info!("收到关闭帧: {:?}", frame);
                            return Ok(());
                        }
                        Message::Frame(_) => {
                            // 原始帧
                        }
                    }
                }
                Err(e) => {
                    error!("接收消息错误: {}", e);
                    return Err(YueError::CustomError(format!("接收消息错误: {}", e)));
                }
            }
        }

        Ok(())
    }
}
