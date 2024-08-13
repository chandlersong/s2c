use crate::models::{Decimal, SwapBalance, UnixTimeStamp};
use crate::utils;
use serde::{Deserialize, Serialize};


#[derive(Debug)]
pub(crate) enum BinanceBase {
    Normal,
    PortfolioMargin,
    SWAP,
}


impl From<BinanceBase> for String {
    fn from(url: BinanceBase) -> Self {
        String::from(
            match url {
                BinanceBase::Normal => String::from("https://api.binance.com/"),
                BinanceBase::PortfolioMargin => String::from("https://papi.binance.com/"),
                BinanceBase::SWAP => String::from("https://fapi.binance.com")
            }
        )
    }
}

pub(crate) enum BinancePath {
    Normal(NormalAPI),
    PAPI(PmAPI),
    FAPI(Future),
}

#[derive(Debug)]
pub(crate) enum NormalAPI {
    PingAPI,
    SpotTickerAPI,
}


pub(crate) enum PmAPI { //统一账户
    BalanceAPI,
    SwapPositionAPI,
}

pub(crate) enum Future { //合约账户
    SwapTickerAPI,
}

impl From<BinancePath> for String {
    fn from(api: BinancePath) -> Self {
        String::from(
            match api {
                BinancePath::Normal(route) => match route {
                    NormalAPI::PingAPI => String::from("api/v3/ping"),
                    NormalAPI::SpotTickerAPI => String::from("/api/v3/ticker/price"),
                }
                BinancePath::PAPI(route) => match route {
                    PmAPI::BalanceAPI => String::from("/papi/v1/balance"),
                    PmAPI::SwapPositionAPI => String::from("/papi/v1/um/positionRisk"),
                }
                BinancePath::FAPI(route) => match route {
                    Future::SwapTickerAPI => String::from("/fapi/v2/ticker/price"),
                }
            }
        )
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
    pub notional: Decimal, // 爆仓价格

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
    pub break_even_price: Decimal //表仓位盈亏平衡价
}

impl From<&UMSwapPosition> for SwapBalance {
    fn from(value: &UMSwapPosition) -> Self {
        SwapBalance {
            symbol: value.symbol.clone(),
            position: value.position_amt.clone(),
            cost_price: value.entry_price.clone(),
            unrealized_profit: value.unrealized_profit.clone(),
            price: value.mark_price.clone(),
        }
    }
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
    pub time: Option<UnixTimeStamp>   // 撮合引擎时间,Spot的不存在这个数据
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


#[cfg(test)]
mod tests {
    use crate::clients::binance_models::{BinanceBase, BinancePath, NormalAPI};

    #[test]
    fn test_api_define() {
        assert_eq!("api/v3/ping", String::from(BinancePath::Normal(NormalAPI::PingAPI)));
        assert_eq!("https://api.binance.com/", String::from(BinanceBase::Normal));
    }
}
