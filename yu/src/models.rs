///
/// 主要为了方便在回测中使用，提供一个统一的接口来获取交易对信息
///
#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
pub trait BacktestInstTrait {
    fn inst_id(&self) -> u64;
    fn symbol(&self) -> &str;
    fn status(&self) -> &str;

    fn base_asset(&self) -> &str;

    fn quote_asset(&self) -> &str;

    fn instrument_type(&self) -> &str;

    fn min_trade_size(&self) -> f64;

    fn min_trade_quantity(&self) -> f64;
    fn tick_size(&self) -> f64;
}

pub type BacktestInst = Box<dyn BacktestInstTrait>;
