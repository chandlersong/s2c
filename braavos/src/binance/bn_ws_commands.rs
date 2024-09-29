use crate::binance::bn_models::{deserialize_wx_method, serialize_wx_method, SymbolDepthData, WsMethod};
use crate::utils::SnowyFlakeWrapper;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

const SF: LazyLock<SnowyFlakeWrapper> = LazyLock::new(|| {
    SnowyFlakeWrapper::new()
});

#[derive(Serialize, Deserialize, Debug)]
pub struct WsRequest {
    id: String,
    #[serde(serialize_with = "serialize_wx_method", deserialize_with = "deserialize_wx_method")]
    method: WsMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Vec<String>>
}


impl WsRequest {
    pub fn new(method: WsMethod, params: Option<Vec<String>>) -> WsRequest {
        let id = SF.next_id_string();
        WsRequest {
            id,
            method,
            params
        }
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
enum WsSpotResponse {
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
        let request = WsRequest { id: "abc".to_string(), method: Ping, params: None };
        let expected = "{\"id\":\"abc\",\"method\":\"ping\"}";
        assert_eq!(expected, request.to_json(), "序列化出错")
    }

    #[test]
    fn test_deserialize_spot_ws_response() {
        let _ = setup_logger(Some(LevelFilter::Debug));
        let entities: Vec<WsSpotResponse> = parse_test_json::<Vec<WsSpotResponse>>("tests/data/ws_stream_btc_usdt_depth.json");
        assert_eq!(entities.len(), 1, "{:?}", entities);
        match &entities[0] {
            WsSpotResponse::Depth(v) => {
                assert_eq!(v.symbol, "BTCUSDT", "symbol mismatch");
                assert_eq!(v.bids.len(), 32, "{:?}", v.bids.len());
                assert_eq!(v.asks.len(), 51, "{:?}", v.asks.len());
                print!("{:?}", v);
            }
        }
    }
}
