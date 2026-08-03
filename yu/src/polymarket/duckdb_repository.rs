use crate::duck_db::DuckDBDSProvider;
use crate::errors::YuError;
use crate::polymarket::po::PolyMarketInstrumentPo;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use yue::query_message::DataSourceProviderTrait;
use yue::tools::get_snow_flake_id_u64;

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait PolyMarketInstrumentRepositoryTrait {
    async fn list_instruments(&self) -> Result<Vec<PolyMarketInstrumentPo>, YuError>;

    async fn insert_instrument(&self, po: &PolyMarketInstrumentPo) -> Result<(), YuError>;

    ///
    /// 返回表中id和asset_id的关系。key为asset_id,val为数据库id
    ///
    async fn get_instrument_dictionary(&self) -> Result<HashMap<String, String>, YuError>;
}
#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait PolyMarketHistoryRepositoryTrait {}

pub type PolyMarketInstrumentRepository = Arc<dyn PolyMarketInstrumentRepositoryTrait + Send + Sync>;
pub type PolyMarketHistoryRepository = Arc<dyn PolyMarketHistoryRepositoryTrait + Send + Sync>;

pub fn get_default_instrument_repo() -> PolyMarketInstrumentRepository {
    Arc::new(PolyMarketInstrumentRepositoryImpl::default())
}

pub fn get_instrument_repo(provider: DuckDBDSProvider) -> PolyMarketInstrumentRepository {
    Arc::new(PolyMarketInstrumentRepositoryImpl { provider })
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

impl PolyMarketInstrumentRepositoryImpl {}

#[async_trait]
impl PolyMarketInstrumentRepositoryTrait for PolyMarketInstrumentRepositoryImpl {
    async fn list_instruments(&self) -> Result<Vec<PolyMarketInstrumentPo>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare("SELECT id, series_id, series_slug, event_id, event_slug, market_id, market_slug, asset_id, asset_slug, start_ms, end_ms FROM polymarket_instruments;")?;
        let mut rows = stmt.query([])?;
        let mut res: Vec<PolyMarketInstrumentPo> = Vec::new();
        while let Some(row) = rows.next()? {
            // read string/int fields, handling possible integer types for timestamps
            let id: u64 = row.get(0)?;
            let series_id: String = row.get(1)?;
            let series_slug: String = row.get(2)?;
            let event_id: String = row.get(3)?;
            let event_slug: String = row.get(4)?;
            let market_id: String = row.get(5)?;
            let market_slug: String = row.get(6)?;
            let asset_id: String = row.get(7)?;
            let asset_slug: String = row.get(8)?;
            let start_ms: u64 = match row.get::<usize, u64>(9) {
                Ok(v) => v,
                Err(_) => row.get::<usize, i64>(9).map(|v| v as u64)?,
            };
            let end_ms: u64 = match row.get::<usize, u64>(10) {
                Ok(v) => v,
                Err(_) => row.get::<usize, i64>(10).map(|v| v as u64)?,
            };

            res.push(PolyMarketInstrumentPo {
                id,
                series_id,
                series_slug,
                event_id,
                event_slug,
                market_id,
                market_slug,
                asset_id,
                asset_slug,
                start_ms,
                end_ms,
            });
        }
        Ok(res)
    }

    async fn insert_instrument(&self, po: &PolyMarketInstrumentPo) -> Result<(), YuError> {
        let conn = self.provider.acquire()?;

        // simple SQL-escaping for single quotes
        let esc = |s: &str| s.replace('\'', "''");

        // use provided id if non-zero, otherwise generate one
        let id = if po.id == 0 { get_snow_flake_id_u64() } else { po.id };
        let series_id = esc(&po.series_id);
        let series_slug = esc(&po.series_slug);
        let event_id = esc(&po.event_id);
        let event_slug = esc(&po.event_slug);
        let market_id = esc(&po.market_id);
        let market_slug = esc(&po.market_slug);
        let asset_id = esc(&po.asset_id);
        let asset_slug = esc(&po.asset_slug);
        let start_ms = po.start_ms;
        let end_ms = po.end_ms;

        let insert_sql = format!(
            "INSERT INTO polymarket_instruments(id, series_id, series_slug, event_id, event_slug, market_id, market_slug, asset_id, asset_slug, start_ms, end_ms) VALUES ({}, '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', {}, {});",
            id, series_id, series_slug, event_id, event_slug, market_id, market_slug, asset_id, asset_slug, start_ms, end_ms
        );

        conn.execute(insert_sql.as_str(), [])?;
        Ok(())
    }

    async fn get_instrument_dictionary(&self) -> Result<HashMap<String, String>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare("SELECT id, asset_id FROM polymarket_instruments;")?;
        let mut rows = stmt.query([])?;
        let mut res: HashMap<String, String> = HashMap::new();
        while let Some(row) = rows.next()? {
            // id might be stored as i64/u64/string
            let id_str = if let Ok(v) = row.get::<usize, u64>(0) {
                v.to_string()
            } else if let Ok(v) = row.get::<usize, i64>(0) {
                v.to_string()
            } else if let Ok(v) = row.get::<usize, String>(0) {
                v
            } else {
                continue;
            };
            let asset_id: String = row.get(1)?;
            res.insert(asset_id, id_str);
        }
        Ok(res)
    }
}
