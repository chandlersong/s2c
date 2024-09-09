use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;

pub type UnixTimeStamp = u64;

//不太确定哪个好，就先用这个用于高精度计算
pub type Decimal = rust_decimal::Decimal;


#[derive(Debug)]
pub struct AccountSummary {
    pub usdt_equity: Decimal,
    pub negative_balance: Decimal,
    pub account_pnl: Decimal,
    pub account_equity: Decimal,
    pub um_swap_summary: SwapSummary,
}


#[derive(Debug)]
pub struct SwapSummary {
    pub long_balance: Decimal,
    pub long_pnl: Decimal,
    pub short_balance: Decimal,
    pub short_pnl: Decimal,
    pub balance: Decimal,
    pub pnl: Decimal,
    pub fra_pnl: Decimal, //funding_rate_arbitrage
    pub positions: Vec<SwapPosition>,
}


#[derive(Debug)]
pub struct SwapPosition {
    pub symbol: String,         //交易对
    pub cur_price: Decimal,     //现在价格
    pub avg_price: Decimal,     //成本价
    pub pos_u: Decimal,         //持仓
    pub pnl_u: Decimal,         //仓位
    pub position_amt: Decimal,  //持仓数量
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

