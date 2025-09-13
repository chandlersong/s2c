use mockall::automock;
use yue::binance::spots::KlineFetcher;

use crate::errors::MingLuanError;

///
/// 主要处理各个交易所的数据的更新操作，
/// 不保存任何交易所的具体操作
///
pub(crate) trait KlineUpdate {
    /// 刷新现货K线
    async fn update(&self) -> Result<(), MingLuanError>;
}

#[automock]
pub trait KlineFetcherFactory<T: KlineFetcher> {
    fn create_fetcher(&self) -> T;
}

pub struct DefaultKlineFetcherFactory<T: Default + KlineFetcher> {
    _marker: std::marker::PhantomData<T>,
}

impl<T: Default + KlineFetcher> DefaultKlineFetcherFactory<T> {
    pub fn new() -> Self {
        DefaultKlineFetcherFactory { _marker: std::marker::PhantomData }
    }
}

impl<T: Default + KlineFetcher> KlineFetcherFactory<T> for DefaultKlineFetcherFactory<T> {
    fn create_fetcher(&self) -> T {
        T::default()
    }
}
