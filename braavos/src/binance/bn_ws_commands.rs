use crate::binance::bn_models::WsMethod;
use crate::utils::SnowyFlakeWrapper;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

const SF: LazyLock<SnowyFlakeWrapper> = LazyLock::new(|| {
    SnowyFlakeWrapper::new()
});

#[derive(Serialize, Deserialize, Debug)]
pub struct WsRequest {
    id: String,
    method: String,
}


impl WsRequest {
    pub fn new(method: WsMethod) -> WsRequest {
        let id = SF.next_id_string();
        WsRequest {
            id,
            method: String::from(method),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use crate::binance::bn_models::WsMethod::Ping;
    use crate::binance::bn_ws_commands::WsRequest;

    #[test]
    fn test_ws_request_2_json() {
        let request = WsRequest { id: "abc".to_string(), method: String::from(Ping) };
        let expected = "{\"id\":\"abc\",\"method\":\"ping\"}";
        assert_eq!(expected, request.to_json(), "序列化出错")
    }
}
