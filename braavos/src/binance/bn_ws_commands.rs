use crate::binance::bn_models::SpotWsSubscribe::AllMiniTicker;
use crate::binance::bn_models::WsMethod::SUBSCRIBE;
use crate::binance::bn_models::{deserialize_wx_method, serialize_wx_method, BinanceBase, MiniTicker, StreamAllMiniTickerResponse, SymbolDepthData, TradeRaw, WsCommandResponse, WsMethod};
use crate::tools::SnowyFlakeWrapper;
use crate::websockets::WebSocketClient;
use log::{error, trace};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, LazyLock};
use tokio::sync::{broadcast, OnceCell, RwLock};
use tokio_tungstenite::tungstenite::Message;

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
    trade_tx_map: Arc<RwLock<HashMap<String, broadcast::Sender<TradeRaw>>>>,
    ws_client: WebSocketClient,
}

impl BNSpotWSClient {
    pub async fn new() -> Self {
        Self::new_with_client(get_spot_client().await).await
    }

    pub async fn new_with_client(ws_client: WebSocketClient) -> Self {
        let mut text_message_rx = ws_client.subscribe_text_message_sender().await;
        let res = Self {
            mini_ticker_tx: Arc::new(RwLock::new(None)),
            trade_tx_map: Arc::new(RwLock::new(HashMap::new())),
            ws_client,
        };
        let listener = res.clone();

        tokio::spawn(async move {
            loop {
                if let Ok(msg) = text_message_rx.recv().await {
                    match listener.handler_message(msg).await {
                        Ok(_) => {}
                        _ => {} //TODO 我没想好怎么处理。
                    };
                }
            }
        });
        res
    }

    pub async fn subscribe_all_mini_ticker(&self) -> broadcast::Receiver<MiniTicker> {
        if let Some(sender) = &self.mini_ticker_tx.read().await.deref(){
            return sender.subscribe();
        }

        let params: Option<Vec<String>> = Some(vec![String::from(AllMiniTicker)]);
        let subscribe_request = WsRequest::new(SUBSCRIBE, params);
        match self.ws_client.send(subscribe_request.to_ws_message()).await {
            Ok(_) => {}
            Err(e) => {
                error!("Failed to subscribe to all mini ticker: {}", e);
            }
        }
        
        let (tx, rx) = broadcast::channel(100);
        self.mini_ticker_tx.write().await.replace(tx);
        rx
    }

    pub async fn subscribe_trade(&mut self, symbol: &str) -> broadcast::Receiver<TradeRaw> {
        if let Some(sender) = self.trade_tx_map.read().await.get(symbol) {
            return sender.subscribe();
        };


        let params: Option<Vec<String>> = Some(vec![
            format!("{}@trade", symbol.to_lowercase())
        ]);
        let subscribe_request = WsRequest::new(SUBSCRIBE, params);
        match self.ws_client.send(subscribe_request.to_ws_message()).await {
            Ok(_) => {}
            Err(e) => {
                error!("Failed to subscribe to {} trade: {}", symbol,e);
            }
        }

        let (tx, rx) = broadcast::channel(100);
        self.trade_tx_map.write().await.insert(symbol.to_string(), tx.clone());
        rx
    }


    async fn handler_message(&self, text: String) -> Result<bool, String> {
        let response = serde_json::from_str(&text);
        match response {
            Ok(response) => {
                let entity: WsSpotResponse = response;
                match entity {
                    WsSpotResponse::Depth(v) => {
                        trace!("{:?} at {:?}", v.symbol,v.event_time);
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
                        //因为websocket可能是共享的，其他地方也可能订阅这个消息。
                        if let Some(sender) = self.trade_tx_map.read().await.get(&v.symbol) {
                            sender.send(v).unwrap();
                        }
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

    pub fn to_ws_message(&self) -> Message {
        Message::text(serde_json::to_string(&self).unwrap())
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




#[cfg(test)]
mod tests {
    use crate::binance::bn_models::WsMethod::Ping;
    use crate::binance::bn_ws_commands::{WsRequest, WsSpotResponse};
    use crate::tools::{parse_test_json, setup_logger};
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
