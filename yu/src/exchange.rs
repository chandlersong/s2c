use std::sync::{Arc, RwLock};
use yue::binance::bn_models::common::{HistoryVo, ToRequestBuilder};
use yue::binance::history_data::{HistoryFetcher, MuteHistoryParam};
use yue::models::HistoryInterval;

/// 主要处理各个交易所的数据的更新操作，
/// 不保存任何交易所的具体操作
pub trait HistoryFetcherFactory: Clone {
    type Param: MuteHistoryParam + ToRequestBuilder + Send + Sync + Clone;
    type Output: HistoryVo + Clone;
    type Fetcher: HistoryFetcher<Self::Param, Self::Output> + Send + Sync + 'static;
    fn create_fetcher(&self) -> Self::Fetcher;
}

#[derive(Clone)]
pub struct CloneHistoryFetcherFactory<T, P, O>
where
    T: HistoryFetcher<P, O> + Clone + Send,
    P: MuteHistoryParam + ToRequestBuilder + Send + Sync + Clone,
    O: HistoryVo + Clone,
{
    base: T,
    _maker: std::marker::PhantomData<(T, P, O)>,
}

impl<T, P, O> CloneHistoryFetcherFactory<T, P, O>
where
    T: HistoryFetcher<P, O> + Clone + Send,
    P: MuteHistoryParam + ToRequestBuilder + Send + Sync + Clone,
    O: HistoryVo + Clone,
{
    pub fn new(base: T) -> Self {
        CloneHistoryFetcherFactory {
            base,
            _maker: Default::default(),
        }
    }
}

impl<T, P, O> HistoryFetcherFactory for CloneHistoryFetcherFactory<T, P, O>
where
    T: HistoryFetcher<P, O> + Clone + Send + Sync + 'static,
    P: MuteHistoryParam + ToRequestBuilder + Send + Sync + Clone,
    O: HistoryVo + Clone + Send,
{
    type Param = P;
    type Output = O;
    type Fetcher = T;

    fn create_fetcher(&self) -> T {
        self.base.clone()
    }
}

//
// 交易所信息的交易信息
pub trait ExchangeDashBoard {
    type TradingSymbol;

    fn spot_all_symbols(&self) -> Arc<RwLock<Vec<Self::TradingSymbol>>>;

    fn swap_all_symbols(&self) -> Arc<RwLock<Vec<Self::TradingSymbol>>>;

    fn spot_trading_symbols(&self) -> Vec<Self::TradingSymbol>;

    fn swap_trading_symbols(&self) -> Vec<Self::TradingSymbol>;
    ///
    /// 返回系统中应该保留的的最早的时间戳。
    /// 理论上，早于这个时间戳的数据，不保证存在。
    ///
    fn get_earliest_timestamp(&self, interval: Option<HistoryInterval>) -> Option<u64>;
}
