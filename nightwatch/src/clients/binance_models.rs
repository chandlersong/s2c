use serde::{Deserialize, Serialize};

use crate::models::{Decimal, UnixTimeStamp};

#[derive(Debug, Serialize, Deserialize)]
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


#[derive(Debug, Serialize, Deserialize)]
pub struct UMSwapPosition {
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

#[derive(Debug, Serialize, Deserialize)]
pub struct UMSwapAssert {
    #[serde(rename = "asset")]
    pub asset: String,            // 资产

    #[serde(rename = "crossWalletBalance")]
    pub cross_wallet_balance: Decimal,      // 全仓账户余额

    #[serde(rename = "crossUnPnl")]
    pub cross_un_pnl: Decimal,    // 全仓持仓未实现盈亏

    #[serde(rename = "maintMargin")]
    pub maint_margin: Decimal,   // 维持保证金

    #[serde(rename = "initialMargin")]
    pub initial_margin: Decimal, // 当前所需起始保证金

    #[serde(rename = "positionInitialMargin")]
    pub position_initial_margin: Decimal,  //持仓所需起始保证金(基于最新标记价格)

    #[serde(rename = "openOrderInitialMargin")]
    pub open_order_initial_margin: Decimal, //当前挂单所需起始保证金(基于最新标记价格)

    #[serde(rename = "updateTime")]
    pub update_time: UnixTimeStamp, // 更新时间
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UMSwapBalance {
    #[serde(rename = "tradeGroupId")]
    pub trade_group_id: i32,
    #[serde(rename = "assets")]
    pub assets: Vec<UMSwapAssert>,
    #[serde(rename = "positions")]
    pub positions: Vec<UMSwapPosition>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Ticker {
    #[serde(rename = "symbol")]
    pub symbol: String,       // 交易对
    #[serde(rename = "price")]
    pub price: Decimal,        // 价格
    #[serde(rename = "time")]
    pub time: Option<UnixTimeStamp>   // 撮合引擎时间,Spot的不存在这个数据
}
