use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;

pub type UnixTimeStamp = u64;

//不太确定哪个好，就先用这个用于高精度计算
pub type Decimal = rust_decimal::Decimal;




pub struct AccountBalance {
    pub swap: Vec<SwapBalance>,
}


pub struct SwapBalance {
    pub symbol: String,
    pub position: Decimal,
    pub cost_price: Decimal,
    pub unrealized_profit: Decimal,
    pub price: Decimal,
}


#[derive(Debug, PartialEq, Default)]
pub struct EmptyObject;


impl std::fmt::Display for EmptyObject {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "")
    }
}

impl Serialize for EmptyObject {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let empty_map: HashMap<String, serde_json::Value> = HashMap::new();
        empty_map.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EmptyObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let empty_map: HashMap<String, serde_json::Value> = HashMap::deserialize(deserializer)?;
        if empty_map.is_empty() {
            Ok(EmptyObject {})
        } else {
            Err(de::Error::custom("Expected an empty JSON object"))
        }
    }
}

