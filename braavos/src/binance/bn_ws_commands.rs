use crate::binance::bn_models::{deserialize_wx_method, serialize_wx_method, MiniTicker, StreamAllMiniTickerResponse, SymbolDepthData, TradeRaw, WsCommandResponse, WsMethod};
use crate::tools::SnowyFlakeWrapper;
use async_trait::async_trait;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, trace};
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
    info!("connect to websocket: {}",&url);
    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
    info!("WebSocket handshake has been successfully completed");
    ws_stream.split()
}

#[async_trait]
trait TextMessageHandler {
    async fn handler_message(&mut self, text: String) -> Result<bool, String>;
}

struct SpotTextMessageHandler
where
{
    mini_ticker_handler: MiniTickerHandler,
    trade_handler: TradeHandler
}

#[async_trait]
impl TextMessageHandler for SpotTextMessageHandler
{
    async fn handler_message(&mut self, text: String) -> Result<bool, String> {
        let response = serde_json::from_str(&text);
        match response {
            Ok(response) => {
                let entity: WsSpotResponse = response;
                match entity {
                    WsSpotResponse::Depth(v) => {
                        trace!("{:?} at {:?}", v.symbol,v.event_time);
                    }
                    WsSpotResponse::StreamAllMiniTicker(v) => {
                        self.mini_ticker_handler.handle(v.tickers).await;
                    }
                    WsSpotResponse::SubAllMiniTicker(v) => {
                        self.mini_ticker_handler.handle(v).await;
                    }
                    WsSpotResponse::Trade(v) => {
                        self.trade_handler.handle(v).await;
                    }
                    WsSpotResponse::CommonResponse(v) => {
                        trace!("receive common result {:?}", v.result);
                    }
                }
                Ok(true)
            }
            Err(e) => {
                error!("error deserializing depth: {:?}", e);
                let error_message = format!("Received error context: {}", text);
                error!("{}", &error_message);
                Err(error_message)
            }
        }
    }
}

///
/// 此方法的作用，主要是为了开启一个方法来开启websocket的监听。
/// 但是在BN这一块来说，因为其websocket是分开的。spot，swap这些是完全分开的。
/// 所以来说，这需要不同的处理。
/// 1. 对一些共性的做一些简单的业务处理。比如重连，ping/pong的处理。
/// 2. 对业务做不同级别的抽象。
async fn ws_listen<TH: TextMessageHandler>(ws_read: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>, writer: ShareWsWriter, url: String, mut text_handler: TH) {
    let mut reader = ws_read;
    loop {
        while let Some(message) = reader.next().await {
            if let Ok(msg) = message {
                match msg {
                    Message::Text(txt) => {
                        match text_handler.handler_message(txt).await {
                            //其实没有想好怎么处理好。
                            Ok(_) => {}
                            Err(_) => {}
                        };
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

pub async fn connect_and_listen(url: String) -> BinanceSpotWSClient {
    let (mini_ticker_tx, _) = broadcast::channel(500);
    /*
    在思考了之后，我绝对，整个client只是负责保存channel。
    然后通过channel对这个websocket做通行。这样比较符合websocket的处理方式
    */
    let (write, read) = connect(url.clone()).await;
    let share_writer = Arc::new(Mutex::new(write));
    let (command_tx, command_rx) = mpsc::channel(1);
    let (trade_tx, trade_rx) = mpsc::channel(100);
    let res = BinanceSpotWSClient::new(command_tx, mini_ticker_tx.clone(), trade_rx);
    let send_clone = Arc::clone(&share_writer);
    tokio::spawn(async move {
        do_send(command_rx, send_clone).await;
    });
    let listen_clone = Arc::clone(&share_writer);
    let mini_ticker_handler = MiniTickerHandler::new(mini_ticker_tx);
    let text_handler = SpotTextMessageHandler {
        mini_ticker_handler,
        trade_handler: TradeHandler { trade_tx },
    };
    tokio::spawn(async move {
        ws_listen(read, listen_clone, url, text_handler).await;
    });

    res
}

///
/// 对外的结构
#[derive(Clone)]
pub struct BinanceSpotWSClient {
    command_tx: Sender<(Message, oneshot::Sender<String>)>,
    pub mini_ticker_tx: broadcast::Sender<MiniTicker>,
    trade_broadcast: Arc<RwLock<HashMap<String, broadcast::Sender<TradeRaw>>>>,
}


impl BinanceSpotWSClient {
    pub fn new(command_tx: Sender<(Message, oneshot::Sender<String>)>,
               mini_ticker_tx: broadcast::Sender<MiniTicker>,
               mut trade_rx: Receiver<TradeRaw>) -> BinanceSpotWSClient {
        let trade_broadcast: Arc<RwLock<HashMap<String, broadcast::Sender<TradeRaw>>>> = Arc::new(RwLock::new(HashMap::new()));
        let refresh = trade_broadcast.clone();
        tokio::spawn(async move {
            loop {
                if let Some(v) = trade_rx.recv().await {
                    let symbol = v.symbol.clone();
                    {
                        let read_guard = refresh.read().await;
                        if let Some(s) = read_guard.get(&symbol).clone() {
                            s.send(v).unwrap();
                            continue;
                        }
                        // 读锁自动释放
                    }
                    {
                        let mut writer_guard = refresh.write().await;
                        let (tx, _) = broadcast::channel(100);
                        tx.send(v).unwrap();
                        writer_guard.insert(symbol, tx);
                    }
                }
            }
        });
        BinanceSpotWSClient {
            command_tx,
            mini_ticker_tx,

            trade_broadcast,
        }
    }


    pub async fn get_trade_tx(&mut self, symbol: &String) -> broadcast::Sender<TradeRaw> {
        {
            let read_guard = self.trade_broadcast.read().await;
            if let Some(s) = read_guard.get(symbol).clone() {
                let sender: broadcast::Sender<TradeRaw> = s.clone();
                return sender;
            }
            // 读锁自动释放
        }
        {
            let mut writer_guard = self.trade_broadcast.write().await;
            let (tx, _) = broadcast::channel(1000);
            writer_guard.insert(symbol.clone(), tx.clone());
            tx
        }
    }

    pub async fn send_command(&mut self, req: WsRequest) -> Result<(), String> {
        let request_body = req.to_json();
        trace!("request body is {}", request_body);
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
    StreamAllMiniTicker(StreamAllMiniTickerResponse),
    SubAllMiniTicker(Vec<MiniTicker>),
    Trade(TradeRaw),
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

impl MiniTickerHandler
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

pub struct TradeHandler
{
    trade_tx: Sender<TradeRaw>,
}

impl TradeHandler {
    pub fn new(trade_tx: Sender<TradeRaw>) -> Self {
        Self {
            trade_tx
        }
    }

    pub async fn handle(&mut self, trade: TradeRaw) {
        self.trade_tx.send(trade).await.unwrap();
    }
}

#[cfg(test)]
mod tests {
    use crate::binance::bn_models::WsMethod::Ping;
    use crate::binance::bn_tools::create_mock_mini_ticker;
    use crate::binance::bn_ws_commands::{MiniTickerHandler, WsRequest, WsSpotResponse};
    use crate::tools::{parse_test_json, setup_logger};
    use log::LevelFilter;
    use tokio::sync::broadcast;

    #[tokio::test]
    async fn test_mini_ticker_handler_new() {
        let (tx, mut rx) = broadcast::channel(2);

        let mut handler = MiniTickerHandler::new(tx);

        let tickers = vec![
            create_mock_mini_ticker("a".to_string(), 1.0),
        ];

        tokio::spawn(async move {
            handler.handle(tickers).await;
        });


        let actual = rx.recv().await;
        match actual {
            Ok(v) => {
                assert_eq!(v.symbol, String::from("a"));
                assert_eq!(v.close, 1.0);
            }
            Err(_) => {
                assert!(false, "")
            }
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
