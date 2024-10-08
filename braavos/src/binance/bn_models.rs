use crate::models::{Decimal, UnixTimeStamp};
use crate::utils;
use crate::utils::unix_time;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug)]
pub enum BinanceBase {
    Normal,
    PortfolioMargin,
}


impl From<BinanceBase> for String {
    fn from(url: BinanceBase) -> Self {
        String::from(
            match url {
                BinanceBase::Normal => String::from("https://api.binance.com/"),
                BinanceBase::PortfolioMargin => String::from("https://papi.binance.com/")
            }
        )
    }
}

pub enum BinancePath {
    Normal(NormalAPI),
    PAPI(PmAPI),
}

#[derive(Debug)]
pub enum NormalAPI {
    PingAPI,
    SpotTickerAPI,
}


pub enum PmAPI { //统一账户
    BalanceAPI,
    SwapPositionAPI,
}


impl From<BinancePath> for String {
    fn from(api: BinancePath) -> Self {
        String::from(
            match api {
                BinancePath::Normal(route) => match route {
                    NormalAPI::PingAPI => String::from("api/v3/Ping"),
                    NormalAPI::SpotTickerAPI => String::from("/api/v3/ticker/price"),
                }
                BinancePath::PAPI(route) => match route {
                    PmAPI::BalanceAPI => String::from("/papi/v1/balance"),
                    PmAPI::SwapPositionAPI => String::from("/papi/v1/um/positionRisk"),
                }
            }
        )
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum WsMethod {
    Ping,
    Time,
    SUBSCRIBE,
}


// 自定义序列化函数
pub fn serialize_wx_method<S>(shape: &WsMethod, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match shape {
        WsMethod::Ping => serializer.serialize_str("ping"),
        WsMethod::Time => serializer.serialize_str("time"),
        WsMethod::SUBSCRIBE => serializer.serialize_str("SUBSCRIBE"),
    }
}

// 自定义反序列化函数
pub fn deserialize_wx_method<'de, D>(deserializer: D) -> Result<WsMethod, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = String::deserialize(deserializer)?;
    match s.as_str() {
        "ping" => { Ok(WsMethod::Ping) }
        "time" => { Ok(WsMethod::Time) }
        "SUBSCRIBE" => { Ok(WsMethod::SUBSCRIBE) }
        _ => {
            panic!("not found command")
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PMBalance {
    pub asset: String,

    #[serde(rename = "totalWalletBalance")]
    pub total_wallet_balance: Decimal, // 钱包余额 =  全仓杠杆未锁定 + 全仓杠杆锁定 + u本位合约钱包余额 + 币本位合约钱包余额

    #[serde(rename = "crossMarginAsset")]
    pub cross_margin_asset: Decimal, // 全仓资产 = 全仓杠杆未锁定 + 全仓杠杆锁定

    #[serde(rename = "crossMarginBorrowed")]
    pub cross_margin_borrowed: Decimal, // 全仓杠杆借贷

    #[serde(rename = "crossMarginFree")]
    pub cross_margin_free: Decimal, // 全仓杠杆未锁定

    #[serde(rename = "crossMarginInterest")]
    pub cross_margin_interest: Decimal, // 全仓杠杆利息

    #[serde(rename = "crossMarginLocked")]
    pub cross_margin_locked: Decimal, //全仓杠杆锁定

    #[serde(rename = "umWalletBalance")]
    pub um_wallet_balance: Decimal,  // u本位合约钱包余额

    #[serde(rename = "umUnrealizedPNL")]
    pub um_unrealized_pnl: Decimal,     // u本位未实现盈亏

    #[serde(rename = "cmWalletBalance")]
    pub cm_wallet_balance: Decimal,       // 币本位合约钱包余额

    #[serde(rename = "cmUnrealizedPNL")]
    pub cm_unrealized_pnl: Decimal,    // 币本位未实现盈亏

    #[serde(rename = "updateTime")]
    pub update_time: UnixTimeStamp,

    #[serde(rename = "negativeBalance")]
    pub negative_balance: Decimal,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UMSwapAssert {
    pub symbol: String,    // 交易对

    #[serde(rename = "initialMargin")]
    pub initial_margin: Decimal,   // 当前所需起始保证金(基于最新标记价格)

    #[serde(rename = "maintMargin")]
    pub maint_margin: Decimal,     // 维持保证金

    #[serde(rename = "unrealizedProfit")]
    pub unrealized_profit: Decimal,  // 持仓未实现盈亏

    #[serde(rename = "positionInitialMargin")]
    pub position_initial_margin: Decimal,      //持仓所需起始保证金(基于最新标记价格)

    #[serde(rename = "openOrderInitialMargin")]
    pub open_order_initial_margin: Decimal,     // 当前挂单所需起始保证金(基于最新标记价格)

    #[serde(rename = "leverage")]
    pub leverage: Decimal,      // 杠杆倍率

    #[serde(rename = "entryPrice")]
    pub entry_price: Decimal,    // 持仓成本价

    #[serde(rename = "maxNotional")]
    pub max_notional: Decimal,    // 当前杠杆下用户可用的最大名义价值

    #[serde(rename = "bidNotional")]
    pub bid_notional: Decimal,  // 买单净值，忽略

    #[serde(rename = "askNotional")]
    pub ask_notional: Decimal,  // 卖单净值，忽略

    #[serde(rename = "positionSide")]
    pub position_side: String,     // 持仓方向

    #[serde(rename = "positionAmt")]
    pub position_amt: Decimal,         //  持仓数量

    #[serde(rename = "updateTime")]
    pub update_time: UnixTimeStamp,         // 更新时间

    #[serde(rename = "breakEvenPrice")]
    pub break_even_price: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UMSwapPosition {
    #[serde(rename = "entryPrice")]
    pub entry_price: Decimal, // 开仓均价

    #[serde(rename = "leverage", deserialize_with = "utils::str_to_u16")]
    pub leverage: u16, // 当前杠杆倍数

    #[serde(rename = "markPrice")]
    pub mark_price: Decimal,   // 当前标记价格

    #[serde(rename = "maxNotionalValue")]
    pub max_notional_value: Decimal, // 当前杠杆倍数允许的名义价值上限

    #[serde(rename = "positionAmt")]
    pub position_amt: Decimal, // 头寸数量，符号代表多空方向, 正数为多，负数为空

    #[serde(rename = "notional")]
    pub notional: Decimal, // 名义价值

    #[serde(rename = "symbol")]
    pub symbol: String, // 交易对

    #[serde(rename = "unRealizedProfit")]
    pub unrealized_profit: Decimal, // 持仓未实现盈亏

    #[serde(rename = "liquidationPrice")]
    pub liquidation_price: Decimal, // 爆仓价格

    #[serde(rename = "positionSide")]
    pub position_side: String, // 持仓方向

    #[serde(rename = "updateTime")]
    pub update_time: UnixTimeStamp,   // 更新时间

    #[serde(rename = "breakEvenPrice")]
    pub break_even_price: Decimal, //表仓位盈亏平衡价
}

pub struct PMRawAccountData {
    pub account_balance: Vec<PMBalance>,
    pub spot_ticker: Vec<Ticker>,
    pub um_swap_position: Vec<UMSwapPosition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UMSwapBalance {
    #[serde(rename = "tradeGroupId")]
    pub trade_group_id: i32,
    #[serde(rename = "assets")]
    pub assets: Vec<UMSwapAssert>,
    #[serde(rename = "positions")]
    pub positions: Vec<UMSwapAssert>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticker {
    #[serde(rename = "symbol")]
    pub symbol: String,       // 交易对
    #[serde(rename = "price")]
    pub price: Decimal,        // 价格
    #[serde(rename = "time")]
    pub time: Option<UnixTimeStamp>,   // 撮合引擎时间,Spot的不存在这个数据
}


pub struct SecurityInfo {
    pub api_key: String,
    pub api_secret: String,
}

pub struct CommandInfo<'a> {
    pub base: BinanceBase,
    pub path: BinancePath,
    pub security: Option<SecurityInfo>,
    pub client: &'a reqwest::Client,
}


pub struct TimeStampRequest {
    pub timestamp: UnixTimeStamp,
    pub rec_window: u16,
}

impl std::fmt::Display for TimeStampRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "timestamp={}&recvWindow={}", self.timestamp, self.rec_window)
    }
}

impl Default for TimeStampRequest {
    fn default() -> Self {
        TimeStampRequest {
            timestamp: unix_time(),
            rec_window: 5000,
        }
    }
}


#[cfg(test)]
mod tests {
    use crate::binance::bn_models::{BinanceBase, BinancePath, NormalAPI};

    #[test]
    fn test_api_define() {
        assert_eq!("api/v3/Ping", String::from(BinancePath::Normal(NormalAPI::PingAPI)));
        assert_eq!("https://api.binance.com/", String::from(BinanceBase::Normal));
    }
}
