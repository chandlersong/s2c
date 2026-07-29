use crate::duck_db::DuckDBDSProvider;
use crate::errors::YuError;
use crate::okx::duckdb_repository::OkxInstrumentRepositoryTrait;
use crate::polymarket::po::PolyMarketInstrumentPo;
use async_trait::async_trait;
use std::sync::Arc;
use yue::query_message::DataSourceProviderTrait;

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait PolyMarketInstrumentRepositoryTrait {
    async fn list_instruments(&self) -> Result<Vec<PolyMarketInstrumentPo>, YuError>;
}
#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait PolyMarketHistoryRepositoryTrait {}

pub type PolyMarketInstrumentRepository = Arc<dyn PolyMarketInstrumentRepositoryTrait + Send + Sync>;
pub type PolyMarketHistoryRepository = Arc<dyn PolyMarketHistoryRepositoryTrait + Send + Sync>;

pub fn get_default_instrument_repo() -> PolyMarketInstrumentRepository {
    Arc::new(PolyMarketInstrumentRepositoryImpl::default())
}
pub struct PolyMarketInstrumentRepositoryImpl {
    provider: DuckDBDSProvider,
}

impl Default for PolyMarketInstrumentRepositoryImpl {
    fn default() -> Self {
        Self {
            provider: DuckDBDSProvider::default(),
        }
    }
}

#[async_trait]
impl PolyMarketInstrumentRepositoryTrait for PolyMarketInstrumentRepositoryImpl {
    async fn list_instruments(&self) -> Result<Vec<PolyMarketInstrumentPo>, YuError> {
        let conn = self.provider.acquire()?;
        todo!()
    }
}
