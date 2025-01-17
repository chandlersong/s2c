use crate::binance::bn_models::{
    deserialize_wx_method, serialize_wx_method, SymbolDepthData, WsCommandResponse, WsMethod,
};
use crate::utils::SnowyFlakeWrapper;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

const SF: LazyLock<SnowyFlakeWrapper> = LazyLock::new(|| SnowyFlakeWrapper::new());


async fn do_send(mut req_recv: Receiver<(Message, oneshot::Sender<String>)>, mut write: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>) {
    while let Some((req, rx)) = req_recv.recv().await {
        if let Err(e) = write.send(req).await {
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

async fn start_listen(mut ws_read: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>, mut sender: BinanceWSClient) {
    while let Some(message) = ws_read.next().await {
        if let Ok(msg) = message {
            match msg {
                Message::Text(txt) => {
                    info!("Received: {}", txt);
                    let entity: WsSpotResponse = serde_json::from_str(&txt).unwrap();
                    match entity {
                        WsSpotResponse::Depth(v) => {
                            info!("{:?} at {:?}", v.symbol,v.event_time);
                        }
                        a => {
                            error!("Received unexpected: {:?}", a);
                        }
                    }
                }
                Message::Ping(ping) => {
                    // Respond to Ping messages with Pong
                    let ping_text = String::from_utf8_lossy(&ping).to_string();
                    info!("收到ping消息:{:?}", ping_text);
                    if sender.send_message(Message::Pong(ping)).await.is_err() {
                        error!("Failed to send Pong");
                        return;
                    }
                    info!("发送pong消息");
                }
                Message::Pong(_) => {
                    // Optionally handle Pong messages
                    println!("Received Pong");
                }
                Message::Close(_) => {
                    // Handle close messages if needed
                    info!("Received Close message");
                    return;
                }
                a => {
                    info!("收到其他消息,{:?}",a);
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct BinanceWSClient {
    req_sender: Sender<(Message, oneshot::Sender<String>)>,
}

impl BinanceWSClient {
    pub async fn connect_and_listen() -> Self {
        /*
        在思考了之后，我绝对，整个client只是负责保存channel。
        然后通过channel对这个websocket做通行。这样比较符合websocket的处理方式
        */
        let url = "wss://stream.binance.com:9443/ws/bbb";
        let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
        info!("WebSocket handshake has been successfully completed");
        let (write, read): (SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>, SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>) = ws_stream.split();
        let (tx, rx) = mpsc::channel(1);
        let res = BinanceWSClient { req_sender: tx };
        let client = res.clone();
        tokio::spawn(async move {
            do_send(rx, write).await;
        });

        tokio::spawn(async move {
            start_listen(read, client).await;
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
        if self.req_sender.send((msg, response_tx)).await.is_err() {
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
}

#[cfg(test)]
mod tests {
    use crate::binance::bn_models::WsMethod::Ping;
    use crate::binance::bn_ws_commands::{WsRequest, WsSpotResponse};
    use crate::utils::{parse_test_json, setup_logger};
    use log::LevelFilter;

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
