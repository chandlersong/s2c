use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use crate::models::UnixTimeStamp;

pub(crate) mod binance;

trait Exchange {
    async fn ping(&self) -> Result<(), reqwest::Error> ;

    async fn get_get_balance(&self) -> Result<Vec<PMBalance>, reqwest::Error> ;
}



#[derive(Debug, Serialize, Deserialize)]
struct PMBalance {
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
    pub update_time:  UnixTimeStamp,
    #[serde(rename = "negativeBalance")]
    pub negative_balance: Decimal,
}
