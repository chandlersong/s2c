use crate::binance::bn_models::SpotWsSubscribe::AllMiniTicker;
use crate::binance::bn_models::WsMethod::SUBSCRIBE;
use crate::binance::bn_models::{deserialize_wx_method, serialize_wx_method, BinanceBase, MiniTicker, SpotDepthData, StreamAllMiniTickerResponse, TradeRaw, WsCommandResponse, WsMethod};
use crate::tools::SnowyFlakeWrapper;
use crate::websockets::WebSocketClient;
use log::{error, info, trace};
use maester::endless_select;
use maester::tools::bus::Bus;
use maester::tools::endless::endless_stop_tx;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ops::Deref;
use std::sync::{Arc, LazyLock};
use tokio::sync::{broadcast, OnceCell, RwLock};
use tokio_tungstenite;
use tokio_tungstenite::tungstenite;

static BN_SPOT_WS_CLIENT: OnceCell<WebSocketClient> = OnceCell::const_new();


async fn initial_spot_ws_client() -> WebSocketClient {
    let url = format!("{}ws/spot",String::from(BinanceBase::WsSwapStreamUrl));
    WebSocketClient::new(&url, None).await.unwrap()
}

pub async fn get_spot_client() -> WebSocketClient {
    BN_SPOT_WS_CLIENT.get_or_init(initial_spot_ws_client).await.clone()
}

const SF: LazyLock<SnowyFlakeWrapper> = LazyLock::new(|| SnowyFlakeWrapper::new());

#[derive(Clone)]
pub struct BNSpotWSClient {
    mini_ticker_tx: Arc<RwLock<Option<broadcast::Sender<MiniTicker>>>>,
    trade_bus: Arc<Bus<TradeRaw>>,
    depth_bus: Arc<Bus<SpotDepthData>>,
    ws_client: WebSocketClient,
    subscribe_item: Arc<RwLock<HashSet<String>>>,
    connected_tx: broadcast::Sender<()>,
}

impl BNSpotWSClient {
    pub async fn new() -> Self {
        Self::new_with_client(get_spot_client().await).await
    }

    pub async fn new_with_client(ws_client: WebSocketClient) -> Self {
        let mut text_message_rx = ws_client.subscribe_text_message_sender().await;
        let connected_tx = ws_client.subscribe_connected();
        let res = Self {
            mini_ticker_tx: Arc::new(RwLock::new(None)),
            trade_bus: Bus::new(),
            depth_bus: Bus::new(),
            ws_client,
            subscribe_item: Arc::new(RwLock::new(HashSet::new())),
            connected_tx: connected_tx.clone(),
        };
        let listener = res.clone();


        endless_select!(
             m = text_message_rx.recv()=>{
                if let Ok(msg) = m{
                    match listener.handler_message(msg).await {
                        Ok(_) => {}
                        _ => {} //TODO 我没想好怎么处理。
                    };
                }

            }
        );


        let mut connected_rx = connected_tx.subscribe();
        _ = connected_rx.recv().await;
        info!("spot websocket 连接成功");
        let re_connected_client = res.clone();
        endless_select!(
             _ = connected_rx.recv()=>{
                info!("连接成功，订阅信息");
                re_connected_client.subscribe_item().await;
            }
        );

        res
    }

    pub fn subscribe_connected(&self) -> broadcast::Sender<()> {
        self.connected_tx.clone()
    }

    pub async fn subscribe_item(&self) {
        info!("开始订阅");
        let subscribe_item = &*self.subscribe_item.read().await;
        for item in subscribe_item {
            info!("订阅，{}", item);
        }
        let params: Option<Vec<String>> = Some(subscribe_item.iter().cloned().collect());
        let subscribe_request = WsRequest::new(SUBSCRIBE, params);
        match self.ws_client.send(subscribe_request.to_ws_message()).await {
            Ok(_) => {}
            Err(e) => {
                error!("Failed to subscribe to all mini ticker: {}", e);
            }
        }
    }

    pub async fn subscribe_all_mini_ticker(&mut self) -> broadcast::Receiver<MiniTicker> {
        if let Some(sender) = &self.mini_ticker_tx.read().await.deref() {
            return sender.subscribe();
        }

        self.subscribe_item.write().await.insert(String::from(AllMiniTicker));
        self.subscribe_item().await;
        
        let (tx, rx) = broadcast::channel(100);
        self.mini_ticker_tx.write().await.replace(tx);
        rx
    }

    pub async fn subscribe_trade(&mut self, symbol: &str) -> broadcast::Receiver<TradeRaw> {
        let subscribe_item = self.trade_bus.subscribe(symbol).await;
        if subscribe_item.is_new {
            let command = format!("{}@trade", symbol.to_lowercase());

            info!("Subscribing subscribe_trade to {}", command);
            self.subscribe_item.write().await.insert(command);
            self.subscribe_item().await;
        };
        subscribe_item.rx
    }


    ///
    /// frequency只能是100或者1000
    pub async fn subscribe_depth(&mut self, symbol: &str, level: u8, frequency: u16) -> broadcast::Receiver<SpotDepthData> {
        let subscribe_item = self.depth_bus.subscribe(symbol).await;
        if subscribe_item.is_new {
            let subscribe_command;
            if frequency == 1000 {
                subscribe_command = format!("{}@depth{}", symbol.to_lowercase(), level);
            } else {
                subscribe_command = format!("{}@depth{}@100ms", symbol.to_lowercase(), level);
            }
            info!("Subscribing depth to {}", subscribe_command);
            self.subscribe_item.write().await.insert(subscribe_command);
            self.subscribe_item().await;
        };
        subscribe_item.rx
    }


    async fn handler_message(&self, text: String) -> Result<bool, String> {
        let response = serde_json::from_str(&text);
        match response {
            Ok(response) => {
                let entity: WsSpotResponse = response;
                match entity {
                    WsSpotResponse::Depth(v) => {
                        trace!("{:?} at {:?}", v.symbol,v.event_time);
                        self.depth_bus.publish(&v.symbol, v.clone()).await;
                    }
                    WsSpotResponse::StreamAllMiniTicker(v) => {
                        if let Some(sender) = self.mini_ticker_tx.read().await.as_ref() {
                            for t in v.tickers {
                                sender.send(t).unwrap();
                            }
                        }
                    }
                    WsSpotResponse::SubAllMiniTicker(v) => {
                        if let Some(sender) = self.mini_ticker_tx.read().await.as_ref() {
                            for t in v {
                                sender.send(t).unwrap();
                            }
                        }
                    }
                    WsSpotResponse::Trade(v) => {
                        self.trade_bus.publish(&v.symbol, v.clone()).await;
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

    pub fn to_ws_message(&self) -> tungstenite::protocol::Message {
        tungstenite::protocol::Message::text(serde_json::to_string(&self).unwrap())
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum WsSpotResponse {
    CommonResponse(WsCommandResponse),
    Depth(SpotDepthData),
    StreamAllMiniTicker(StreamAllMiniTickerResponse),
    SubAllMiniTicker(Vec<MiniTicker>),
    Trade(TradeRaw),
}




#[cfg(test)]
mod tests {
    use crate::binance::bn_models::WsMethod::Ping;
    use crate::binance::bn_ws_commands::{WsRequest, WsSpotResponse};
    use crate::tools::parse_test_json;
    use log::LevelFilter;
    use maester::tools::logs::setup_logger;

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
