use crate::binance::bn_cache::{refresh_cache, ShareCache};
use crate::binance::bn_models::{deserialize_wx_method, serialize_wx_method, AllMiniTickerResponse, MiniTicker, SymbolDepthData, WsCommandResponse, WsMethod};
use crate::utils::SnowyFlakeWrapper;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex, RwLock};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

const SF: LazyLock<SnowyFlakeWrapper> = LazyLock::new(|| SnowyFlakeWrapper::new());

type ShareWsWriter = Arc<Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>;
async fn do_send(mut req_recv: Receiver<(Message, oneshot::Sender<String>)>, write: ShareWsWriter) {
    while let Some((req, rx)) = req_recv.recv().await {
        if let Err(e) = write.lock().await.send(req).await {
            error!("error sending request to BN: {}",  e);
            match rx.send("fail".to_string()) {
                Ok(_) => {}
                Err(e) => { error!("error while sending message: {}",  e); }
            }
        } else {
            match rx.send("ok".to_string()) {
                Ok(_) => {}
                Err(e) => { error!("error while sending message: {}",  e); }
            }
        }
    }
}

pub async fn connect(url: String) -> (SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>, SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>) {
    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
    info!("WebSocket handshake has been successfully completed");
    ws_stream.split()
}

async fn start_listen(ws_read: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>, writer: ShareWsWriter, url: String, mut mini_ticker_handler: MiniTickerHandler) {
    let mut reader = ws_read;
    loop {
        while let Some(message) = reader.next().await {
            if let Ok(msg) = message {
                match msg {
                    Message::Text(txt) => {
                        let response = serde_json::from_str(&txt);
                        match response {
                            Ok(response) => {
                                let entity: WsSpotResponse = response;
                                match entity {
                                    WsSpotResponse::Depth(v) => {
                                        debug!("{:?} at {:?}", v.symbol,v.event_time);
                                    }
                                    WsSpotResponse::AllMiniTicker(v) => {
                                        debug!("receive mini ticker,num:{:?}", v.tickers.len());
                                        mini_ticker_handler.handle(v.tickers).await;
                                    }
                                    WsSpotResponse::CommonResponse(v) => {
                                        debug!("receive common result {:?}", v.result);
                                    }
                                }
                            }
                            Err(e) => {
                                error!("error deserializing depth: {:?}", e);
                                error!("Received error context: {}", txt);
                            }
                        }
                    }
                    Message::Ping(ping) => {
                        // Respond to Ping messages with Pong
                        let ping_text = String::from_utf8_lossy(&ping).to_string();
                        debug!("收到ping消息:{:?}", ping_text);
                        if writer.lock().await.send(Message::Pong(ping)).await.is_err() {
                            error!("Failed to send Pong");
                            return;
                        }
                        debug!("发送pong消息");
                    }
                    Message::Pong(_) => {
                        // Optionally handle Pong messages
                        debug!("Received Pong");
                    }
                    Message::Close(_) => {
                        // Handle close messages if needed
                        info!("Received Close message");
                        break;
                    }
                    a => {
                        info!("收到其他消息,{:?}",a);
                    }
                }
            }
        }
        info!("ws断开，重新连接");
        let (hew_writer, new_read) = connect(url.clone()).await;

        reader = new_read;
        let mut writer_pr = writer.lock().await;
        *writer_pr = hew_writer;
        info!("ws断开，重新连接完成");
    }
}

///
/// 对外的结构
#[derive(Clone)]
pub struct BinanceWSClient {
    command_tx: Sender<(Message, oneshot::Sender<String>)>,
    pub mini_ticker_tx: broadcast::Sender<MiniTicker>,
}


impl BinanceWSClient {
    pub async fn connect_and_listen(url: String) -> Self {
        let (mini_ticker_tx, _) = broadcast::channel(500);
        /*
        在思考了之后，我绝对，整个client只是负责保存channel。
        然后通过channel对这个websocket做通行。这样比较符合websocket的处理方式
        */
        let (write, read) = connect(url.clone()).await;
        let share_writer = Arc::new(Mutex::new(write));
        let (command_tx, command_rx) = mpsc::channel(1);
        let res = BinanceWSClient { command_tx, mini_ticker_tx: mini_ticker_tx.clone() };
        let send_clone = Arc::clone(&share_writer);
        tokio::spawn(async move {
            do_send(command_rx, send_clone).await;
        });
        let listen_clone = Arc::clone(&share_writer);
        let mini_ticker_handler = MiniTickerHandler {
            mini_ticker_tx: mini_ticker_tx.clone(),
        };
        tokio::spawn(async move {
            start_listen(read, listen_clone, url, mini_ticker_handler).await;
        });

        res
    }


    pub async fn send_command(&mut self, req: WsRequest) -> Result<(), String> {
        let request_body = req.to_json();
        debug!("request body is {}", request_body);
        self.send_message(Message::Text(request_body)).await
    }

    pub async fn send_message(&mut self, msg: Message) -> Result<(), String> {
        let (response_tx, response_rx) = oneshot::channel();
        if self.command_tx.send((msg, response_tx)).await.is_err() {
            Err("websocket may be close".to_string())
        } else {
            match timeout(Duration::from_secs(2), response_rx).await {
                Ok(response_result) => match response_result {
                    Ok(_) => Ok(()),
                    Err(_) => Err("websocket closed".to_string()),
                },
                Err(_) => Err("connection time out".to_string()),
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WsRequest {
    id: String,
    #[serde(
        serialize_with = "serialize_wx_method",
        deserialize_with = "deserialize_wx_method"
    )]
    method: WsMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Vec<String>>,
}

impl WsRequest {
    pub fn new(method: WsMethod, params: Option<Vec<String>>) -> WsRequest {
        let id = SF.next_id_string();
        WsRequest { id, method, params }
    }

    pub fn empty_new(method: WsMethod) -> WsRequest {
        let id = SF.next_id_string();
        WsRequest {
            id,
            method,
            params: None,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum WsSpotResponse {
    CommonResponse(WsCommandResponse),
    Depth(SymbolDepthData),
    AllMiniTicker(AllMiniTickerResponse),
}


/**
* 这里的代码的主要作用还是为了把消息的处理单独抽离出来。
* 其实之后的想法，每一个需要订阅的消息。都会有一个专门的处理类
*/
pub struct MiniTickerHandler
where
{
    mini_ticker_tx: broadcast::Sender<MiniTicker>,
}

impl<> MiniTickerHandler<>
{
    pub fn new(mini_ticker_tx: broadcast::Sender<MiniTicker>) -> Self {
        MiniTickerHandler {
            mini_ticker_tx
        }
    }

    pub async fn handle(&mut self, tickers: Vec<MiniTicker>) {
        for ticker in tickers {
            self.mini_ticker_tx.send(ticker).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::binance::bn_models::MiniTicker;
    use crate::binance::bn_models::WsMethod::Ping;
    use crate::binance::bn_ws_commands::{WsRequest, WsSpotResponse};
    use crate::utils::{parse_test_json, setup_logger};
    use log::LevelFilter;


    fn create_mock_mini_ticker(symbol: String, val: f64) -> MiniTicker {
        MiniTicker {
            event_type: "abc".to_string(),
            event_time: 0,
            symbol,
            close: val,
            open: val,
            high: val,
            low: val,
            volume: val,
            quote_volume: val,
        }
    }

    #[test]
    fn test_ws_request_2_json() {
        let request = WsRequest {
            id: "abc".to_string(),
            method: Ping,
            params: None,
        };
        let expected = "{\"id\":\"abc\",\"method\":\"ping\"}";
        assert_eq!(expected, request.to_json(), "序列化出错")
    }

    #[test]
    fn test_deserialize_spot_ws_response() {
        let _ = setup_logger(Some(LevelFilter::Debug));
        let entities: Vec<WsSpotResponse> =
            parse_test_json::<Vec<WsSpotResponse>>("tests/data/ws_stream_btc_usdt_depth.json");
        assert_eq!(entities.len(), 1, "{:?}", entities);
        match &entities[0] {
            WsSpotResponse::Depth(v) => {
                assert_eq!(v.symbol, "BTCUSDT", "symbol mismatch");
                assert_eq!(v.bids.len(), 32, "{:?}", v.bids.len());
                assert_eq!(v.asks.len(), 51, "{:?}", v.asks.len());
                print!("{:?}", v);
            }
            _ => {}
        }
    }

}
