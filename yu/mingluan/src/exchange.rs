use std::sync::{Arc, RwLock};
use yue::binance::spots::KlineFetcher;

/// 主要处理各个交易所的数据的更新操作，
/// 不保存任何交易所的具体操作
pub trait KlineFetcherFactory: Clone {
    type Fetcher: KlineFetcher + Send; //
    fn create_fetcher(&self) -> Self::Fetcher;
}

#[derive(Clone)]
pub struct DefaultKlineFetcherFactory<T: Default + KlineFetcher> {
    _marker: std::marker::PhantomData<T>,
}

impl<T: Default + KlineFetcher> DefaultKlineFetcherFactory<T> {
    pub fn new() -> Self {
        DefaultKlineFetcherFactory { _marker: std::marker::PhantomData }
    }
}

impl<T: Default + KlineFetcher + Clone + Send> KlineFetcherFactory for DefaultKlineFetcherFactory<T> {
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
