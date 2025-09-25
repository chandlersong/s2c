use std::sync::{Arc, RwLock};
use yue::binance::bn_models::{HistoryVo, ToQueryParams};
use yue::binance::history_data::{HistoryFetcher, MuteHistoryParam};

/// 主要处理各个交易所的数据的更新操作，
/// 不保存任何交易所的具体操作
pub trait HistoryFetcherFactory: Clone {
    type Param: MuteHistoryParam + ToQueryParams + Send + Sync + Clone;
    type Output: HistoryVo + Clone;
    type Fetcher: HistoryFetcher<Self::Param, Self::Output> + Send + Sync + 'static;
    fn create_fetcher(&self) -> Self::Fetcher;
}

#[derive(Clone)]
pub struct DefaultHistoryFetcherFactory<T, P, O>
where
    T: Default + HistoryFetcher<P, O> + Clone + Send,
    P: MuteHistoryParam + ToQueryParams + Send + Sync + Clone,
    O: HistoryVo + Clone,
{
    _marker: std::marker::PhantomData<(T, P, O)>,
}

impl<T, P, O> DefaultHistoryFetcherFactory<T, P, O>
where
    T: Default + HistoryFetcher<P, O> + Clone + Send,
    P: MuteHistoryParam + ToQueryParams + Send + Sync + Clone,
    O: HistoryVo + Clone,
{
    pub fn new() -> Self {
        DefaultHistoryFetcherFactory {
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T, P, O> HistoryFetcherFactory for DefaultHistoryFetcherFactory<T, P, O>
where
    T: Default + HistoryFetcher<P, O> + Clone + Send + Sync + 'static,
    P: MuteHistoryParam + ToQueryParams + Send + Sync + Clone,
    O: HistoryVo + Clone + Send,
{
    type Param = P;
    type Output = O;
    type Fetcher = T;

    fn create_fetcher(&self) -> T {
        T::default()
    }
}

//
// 交易所信息的交易信息
pub trait ExchangeDashBoard {
    type SpotDashBoard;

    fn spot_info(&self) -> Arc<RwLock<Self::SpotDashBoard>>;
}
