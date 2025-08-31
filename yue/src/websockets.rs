use crate::errors::YueError;
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, trace};
use std::error::Error;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast, mpsc, oneshot};
use tokio::time::{Duration, sleep, timeout};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[derive(Debug, Clone)]
pub enum ResponseCode {
    Ok,
    Failed,
    TimedOut,
}

#[derive(Clone)]
pub struct WebSocketConfig {
    reconnect_duration: Duration,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            reconnect_duration: Duration::from_secs(5),
        }
    }
}

type TextMessageSender = Arc<RwLock<Option<broadcast::Sender<String>>>>;
#[derive(Clone)]
pub struct WebSocketClient {
    url: String,
    text_message_tx: Arc<RwLock<Option<broadcast::Sender<String>>>>,
    command_tx: mpsc::Sender<(Message, oneshot::Sender<ResponseCode>)>,
    config: WebSocketConfig,
    connected_tx: broadcast::Sender<()>,
}

impl WebSocketClient {
    pub async fn new(url: &str, config: Option<WebSocketConfig>) -> Result<Self, Box<dyn Error>> {
        let (tx, rx) = mpsc::channel::<(Message, oneshot::Sender<ResponseCode>)>(1000);
        let (connected_tx, _) = broadcast::channel(100);
        let real_config = config.unwrap_or_default();
        let client = WebSocketClient {
            url: url.to_string(),
            text_message_tx: Arc::new(RwLock::new(None)),
            command_tx: tx,
            config: real_config,
            connected_tx,
        };

        client.start_connection(rx).await;
        Ok(client)
    }

    pub fn subscribe_connected(&self) -> broadcast::Sender<()> {
        self.connected_tx.clone()
    }

    pub async fn subscribe_text_message_sender(&self) -> broadcast::Receiver<String> {
        if let Some(sender) = self.text_message_tx.read().await.as_ref() {
            return sender.subscribe();
        };

        let (tx, rx) = broadcast::channel(100);
        *self.text_message_tx.write().await = Some(tx);
        rx
    }

    async fn start_connection(
        &self,
        mut rx: mpsc::Receiver<(Message, oneshot::Sender<ResponseCode>)>,
    ) {
        let url = self.url.clone();
        let tx = self.command_tx.clone();
        let reconnect_duration = self.config.reconnect_duration.clone();
        let text_message_tx = self.text_message_tx.clone();

        let connected_tx = self.connected_tx.clone();
        tokio::spawn(async move {
            loop {
                match Self::run_connection(&url, &text_message_tx, &tx, &mut rx, &connected_tx)
                    .await
                {
                    Ok(()) => info!("WebSocket closed normally"),
                    Err(e) => error!("WebSocket error: {}", e),
                }
                info!("Reconnecting in 5 seconds...");
                sleep(reconnect_duration).await;
            }
        });
    }

    async fn run_connection(
        url: &str,
        text_message_tx: &TextMessageSender,
        _: &mpsc::Sender<(Message, oneshot::Sender<ResponseCode>)>, //为扩展准备。可以发送命令
        command: &mut mpsc::Receiver<(Message, oneshot::Sender<ResponseCode>)>,
        notify: &broadcast::Sender<()>,
    ) -> Result<(), Box<dyn Error>> {
        let (ws_stream, _) = connect_async(url).await?;
        info!("WebSocket connected to {}", url);
        let (mut write, mut read) = ws_stream.split();
        //觉得没有什么好处理的。错误也是别人没有听到。
        match notify.send(()) {
            Ok(_) => {}
            Err(_) => {}
        }
        let ping_interval = Duration::from_secs(10);
        loop {
            tokio::select! {
                    // 处理接收消息
                    Some(msg) = read.next() => {
                        match msg {
                            Ok(Message::Text(text)) => {
                                if let Some(sender)= text_message_tx.read().await.as_ref() {
                                    match sender.send(text){
                                        Ok(..) => {},
                                        Err(e) => error!("发送文字消息失败: {}", e),
                                    };
                                };
                            }
                            Ok(Message::Ping(data)) => {
                                trace!("Received Ping: {:?}", data);
                            }
                            Ok(Message::Pong(_)) => {
                                 trace!("Received Pong");
                            }
                            Ok(Message::Close(_)) => {
                                debug!("Server closed connection");
                                return Ok(());
                            }
                            Ok(_) => {} // 忽略其他类型
                            Err(e) => {
                                return Err(Box::new(e));
                            }
                        }

                    }

                // 处理发送消息或 Ping
                Some((msg, tx)) = command.recv() => {
                    match timeout(Duration::from_secs(1), write.send(msg)).await {
                        Ok(result) => match result {
                            Ok(_) => {
                                match tx.send(ResponseCode::Ok) {
                                    Ok(_) => {}
                                    Err(e) => { error!("error while sending ok message: {:?}",  e); }
                                }
                            }
                            Err(e) => {
                                    error!("error while sending ok message: {:?}", e);
                                    match tx.send(ResponseCode::Failed) {
                                        Ok(_) => {}
                                        Err(e) => { error!("error while sending error notification: {:?}",  e); }
                                    }
                            }
                        },
                        Err(_) => {
                            match tx.send(ResponseCode::TimedOut) {
                                Ok(_) => {}
                                Err(e) => { error!("error while sending timeout notification: {:?}",  e); }
                            }
                        }
                    }
                }
                _ = sleep(ping_interval) => {
                    let ping = Message::Ping(vec![1, 2, 3]);
                    write.send(ping).await?;
                    debug!("Sent Ping");
                }
            }
        }
    }

    pub async fn send(&self, message: Message) -> Result<(), Box<dyn Error>> {
        let (tx, rx) = oneshot::channel();
        self.command_tx.send((message, tx)).await?;

        match rx.await {
            Ok(result) => match result {
                ResponseCode::Ok => Ok(()),
                ResponseCode::Failed => Err(Box::new(YueError::new("request error"))),
                ResponseCode::TimedOut => Err(Box::new(YueError::new("request timeout error"))),
            },
            Err(error) => {
                error!("request error: {}", error);
                Err(Box::new(YueError::new("request error")))
            }
        }
    }
}
