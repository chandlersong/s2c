use crate::binance::bn_models::bin::Trade;
use crate::binance::bn_models::{deserialize_wx_method, serialize_wx_method, BinanceBase, MiniTicker, StreamAllMiniTickerResponse, SymbolDepthData, TradeRaw, WsCommandResponse, WsMethod};
use crate::tools::SnowyFlakeWrapper;
use crate::websockets::WebSocketClient;
use async_trait::async_trait;
use futures_util::stream::SplitStream;
use futures_util::SinkExt;
use log::{error, trace};
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::sync::{Arc, LazyLock};
use tokio::sync::{broadcast, oneshot, Mutex, OnceCell, RwLock};

static BN_SPOT_WS_CLIENT: OnceCell<WebSocketClient> = OnceCell::const_new();


async fn initial_spot_ws_client() -> WebSocketClient {
    let url = String::from(BinanceBase::WsSwapStreamUrl);
    WebSocketClient::new(&url, None).await.unwrap()
}

pub async fn get_spot_client() -> WebSocketClient {
    BN_SPOT_WS_CLIENT.get_or_init(initial_spot_ws_client).await.clone()
}

const SF: LazyLock<SnowyFlakeWrapper> = LazyLock::new(|| SnowyFlakeWrapper::new());

#[derive(Clone)]
pub struct BNSpotWSClient {
    mini_ticker_tx: Arc<RwLock<Option<broadcast::Sender<MiniTicker>>>>,
    trade_tx: Arc<RwLock<Option<broadcast::Sender<TradeRaw>>>>,
}

impl BNSpotWSClient {
    pub async fn new() -> Self {
        let res = Self {
            mini_ticker_tx: Arc::new(RwLock::new(None)),
            trade_tx: Arc::new(RwLock::new(None)),
        };
        let mut text_message_rx = get_spot_client().await.subscribe_text_message_sender().await;
        let mut listener = res.clone();

        tokio::spawn(async move {
            loop {
                if let Ok(msg) = text_message_rx.recv().await {
                   match  listener.handler_message(msg).await{
                       Ok(m) => {}
                       _ => {} //TODO 我没想好怎么处理。
                   } ;
                }
            }
        });
        res
    }

    async fn subscribe_mini_ticker_tx(&self) -> broadcast::Receiver<MiniTicker> {
        if let Some(sender) = &self.mini_ticker_tx.read().await.deref(){
            return sender.subscribe();
        }
        let (tx, rx) = broadcast::channel(100);
        self.mini_ticker_tx.write().await.replace(tx);
        rx
    }

    fn subscribe_all_mini_ticker(&mut self) -> broadcast::Receiver<MiniTicker> {
        let (tx, rx) = broadcast::channel(10);
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
                        if let Some(sender) = self.trade_tx.read().await.as_ref() {
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
