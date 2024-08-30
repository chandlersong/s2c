use crate::prometheus_gauge;
use crate::prometheus_server::ToGauge;
use prometheus::Gauge;
use rust_decimal_macros::dec;
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

impl ToGauge for AccountSummary {
    fn to_prometheus_gauge(&self, strategy: &str) -> Vec<Gauge> {
        let side_name = format!("{}_acc_detail", strategy);
        let acc_equity = prometheus_gauge!(side_name,self.account_equity,("field" => "acc_equity"));
        let negative_balance = prometheus_gauge!(side_name,self.negative_balance,("field" => "negative_balance"));
        let usdt_equity = prometheus_gauge!(side_name,self.usdt_equity,("field" => "usdt_equity"));
        let account_pnl = prometheus_gauge!(side_name,self.account_pnl,("field" => "account_pnl"));
        vec![acc_equity, negative_balance, usdt_equity, account_pnl]
    }
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

impl ToGauge for SwapSummary {
    fn to_prometheus_gauge(&self, strategy: &str) -> Vec<Gauge> {
        let acc = prometheus_gauge!(format!("{}_acc", strategy),self.balance);
        let pnl = prometheus_gauge!(format!("{}_pnl", strategy),self.pnl);
        let acc_long = prometheus_gauge!(format!("{}_acc_long", strategy),self.long_balance);
        let acc_long_pnl = prometheus_gauge!(format!("{}_acc_long_pnl", strategy),self.long_pnl);
        let acc_short = prometheus_gauge!(format!("{}_acc_short", strategy),self.short_balance);
        let acc_short_pnl = prometheus_gauge!(format!("{}_acc_short_pnl", strategy),self.short_pnl);
        let fra_pnl = prometheus_gauge!(format!("{}_fra_pnl", strategy),self.fra_pnl);
        vec![acc, acc_long, acc_long_pnl, acc_short, acc_short_pnl, pnl, fra_pnl]
    }
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


impl ToGauge for SwapPosition {
    fn to_prometheus_gauge(&self, strategy: &str) -> Vec<Gauge> {
        let side = if self.position_amt > dec!(0) { dec!(1) } else { dec!(-1) };
        let side_name = if side == dec!(1) { format!("{strategy}_long") } else { format!("{strategy}_short") };

        let cur_price = prometheus_gauge!(side_name,self.cur_price,("field" => "cur_price"),("symbol" => &self.symbol));
        let avg_price = prometheus_gauge!(side_name,self.avg_price,("field" => "avg_price"),("symbol" => &self.symbol));
        let pos = prometheus_gauge!(side_name,self.position_amt,("field" => "pos"),("symbol" => &self.symbol));
        let pnl_u = prometheus_gauge!(side_name,self.pnl_u,("field" => "pnl_u"),("symbol" => &self.symbol));
        let value = prometheus_gauge!(side_name,self.pos_u,("field" => "value"),("symbol" => &self.symbol));


        let change_value: Decimal = (self.cur_price / self.avg_price - dec!(1)) * side;
        let change = prometheus_gauge!(side_name,change_value,("field" => "change"),("symbol" => &self.symbol));
        vec![cur_price, pos, pnl_u, avg_price, change, value]
    }
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

#[cfg(test)]
mod tests {
    use crate::models::{AccountSummary, SwapPosition, SwapSummary};
    use crate::prometheus_server::ToGauge;
    use rust_decimal_macros::dec;

    #[test]
    fn test_to_swap_position_prometheus() {
        let swap_position = SwapPosition {
            symbol: "bbb".to_string(),
            cur_price: dec!(1),
            avg_price: dec!(2),
            pos_u: Default::default(),
            pnl_u: Default::default(),
            position_amt: Default::default(),
        };
        let actual = swap_position.to_prometheus_gauge("test");

        assert_eq!(6, actual.len());
    }

    #[test]
    fn test_to_swap_summary_prometheus() {
        let swap_position = SwapSummary {
            long_balance: Default::default(),
            long_pnl: Default::default(),
            short_balance: Default::default(),
            short_pnl: Default::default(),
            balance: Default::default(),
            pnl: Default::default(),
            fra_pnl: Default::default(),
            positions: vec![],
        };
        let actual = swap_position.to_prometheus_gauge("test");

        assert_eq!(7, actual.len());
    }

    #[test]
    fn test_to_account_summary_prometheus() {
        let swap_position = AccountSummary {
            usdt_equity: Default::default(),
            negative_balance: Default::default(),
            account_pnl: Default::default(),
            account_equity: Default::default(),
            um_swap_summary: SwapSummary {
                long_balance: Default::default(),
                long_pnl: Default::default(),
                short_balance: Default::default(),
                short_pnl: Default::default(),
                balance: Default::default(),
                pnl: Default::default(),
                fra_pnl: Default::default(),
                positions: vec![],
            },
        };
        let actual = swap_position.to_prometheus_gauge("test");

        assert_eq!(4, actual.len());
    }
}
