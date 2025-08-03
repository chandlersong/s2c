use braavos::binance::bn_models::{BinanceBase, BinancePath, CommandInfo, NormalAPI};
use braavos::settings::BRAAVOS_SETTING;

///
/// 这个example的主要作用是
/// 1. 展示如果去

#[tokio::main]
async fn main() {

    let setting = &BRAAVOS_SETTING;
    let account = setting.get_account(0);
    let account_query = account.clone();
    let info = CommandInfo::new(BinanceBase::Normal, BinancePath::Normal(NormalAPI::PingAPI));


    // let result = rate_limited!(1, 42).await;
    // assert!(result.is_ok());
    // assert_eq!(result.unwrap(), 42);
    //
    // // 测试高权重调用，触发超时
    // let result = rate_limited!(30, 42).await; // 超过突发容量
    // assert!(result.is_err());
}
