use crate::duck_db::DuckDBDSProvider;
use crate::duck_db_tables::DuckTableTableChannel;
use crate::errors::YuError;
use crate::polymarket::database::get_polymarket_price_history_table;
use crate::polymarket::po::{PolyMarketHistoryPo, PolyMarketInstrumentPo};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::oneshot;
use yue::query_message::{DataSourceProviderTrait, InsertPayload, QueryCommand};
use yue::tools::get_snow_flake_id_u64;

///
/// 查询polymarket_instruments
///
#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait PolyMarketInstrumentRepositoryTrait {
    async fn list_instruments(&self) -> Result<Vec<PolyMarketInstrumentPo>, YuError>;

    async fn insert_instrument(&self, po: &PolyMarketInstrumentPo) -> Result<(), YuError>;

    ///
    /// 返回表中id和asset_id的关系。key为asset_id,val为数据库id
    ///
    async fn get_instrument_dictionary(&self) -> Result<HashMap<String, u64>, YuError>;
}

///
/// 查询polymarket_price_history
///
#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait PolyMarketHistoryRepositoryTrait {
    async fn insert_history(&self, po: PolyMarketHistoryPo) -> Result<(), YuError>;

    ///
    /// 返回表中id和timestamp的关系。key为instrument_id,val为数据库中最大的timestamp
    ///
    async fn max_timestamp_group_by_inst_id(&self) -> Result<HashMap<u64, u64>, YuError>;

    async fn get_history_between(&self, inst_id: u64, start_ms: u64, end_ms: u64) -> Result<Vec<PolyMarketHistoryPo>, YuError>;
}

pub type PolyMarketInstrumentRepository = Arc<dyn PolyMarketInstrumentRepositoryTrait + Send + Sync>;
pub type PolyMarketHistoryRepository = Arc<dyn PolyMarketHistoryRepositoryTrait + Send + Sync>;

pub fn get_instrument_repo(provider: Option<DuckDBDSProvider>) -> PolyMarketInstrumentRepository {
    match provider {
        None => Arc::new(PolyMarketInstrumentRepositoryImpl::default()),
        Some(p) => Arc::new(PolyMarketInstrumentRepositoryImpl { provider: p }),
    }
}

pub fn get_history_repo(
    provider: Option<DuckDBDSProvider>,
    db_channel: Option<DuckTableTableChannel<PolyMarketHistoryPo>>,
) -> PolyMarketHistoryRepository {
    let db_provider = provider.unwrap_or_else(|| DuckDBDSProvider::default());
    let channel = db_channel.unwrap_or_else(|| get_polymarket_price_history_table());
    Arc::new(PolyMarketHistoryRepositoryImpl {
        provider: db_provider,
        channel,
    })
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

    async fn get_instrument_dictionary(&self) -> Result<HashMap<String, u64>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare("SELECT id, asset_id FROM polymarket_instruments;")?;
        let mut rows = stmt.query([])?;
        let mut res: HashMap<String, u64> = HashMap::new();
        while let Some(row) = rows.next()? {
            // id might be stored as i64/u64/string
            let id = if let Ok(v) = row.get::<usize, u64>(0) {
                v
            } else if let Ok(v) = row.get::<usize, i64>(0) {
                v as u64
            } else if let Ok(v) = row.get::<usize, String>(0) {
                v.parse::<u64>().unwrap_or(0)
            } else {
                continue;
            };
            let asset_id: String = row.get(1)?;
            res.insert(asset_id, id);
        }
        Ok(res)
    }
}

pub struct PolyMarketHistoryRepositoryImpl {
    provider: DuckDBDSProvider,
    channel: DuckTableTableChannel<PolyMarketHistoryPo>,
}

impl Default for PolyMarketHistoryRepositoryImpl {
    fn default() -> Self {
        Self {
            provider: DuckDBDSProvider::default(),
            channel: get_polymarket_price_history_table(),
        }
    }
}

#[async_trait]
impl PolyMarketHistoryRepositoryTrait for PolyMarketHistoryRepositoryImpl {
    async fn insert_history(&self, po: PolyMarketHistoryPo) -> Result<(), YuError> {
        let (tx, rx) = oneshot::channel();
        let command = QueryCommand::Insert(InsertPayload::new(po, tx));
        self.channel
            .send(command)
            .await
            .map_err(|e| YuError::new(&format!("Failed to send command: {}", e)))?;
        match rx.await {
            Ok(_) => Ok(()),
            Err(e) => Err(YuError::new(&format!("Failed to receive response: {}", e))),
        }
    }

    async fn max_timestamp_group_by_inst_id(&self) -> Result<HashMap<u64, u64>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare("SELECT instrument_id, max(timestamp) FROM polymarket_price_history GROUP BY instrument_id;")?;
        let mut rows = stmt.query([])?;
        let mut res: HashMap<u64, u64> = HashMap::new();
        while let Some(row) = rows.next()? {
            // instrument_id may be stored as u64/i64/string
            let id = if let Ok(v) = row.get::<usize, u64>(0) {
                v
            } else if let Ok(v) = row.get::<usize, i64>(0) {
                v as u64
            } else if let Ok(v) = row.get::<usize, String>(0) {
                match v.parse::<u64>() {
                    Ok(n) => n,
                    Err(_) => continue,
                }
            } else {
                continue;
            };

            // max(timestamp) may be numeric or string
            let max_ts: u64 = if let Ok(v) = row.get::<usize, u64>(1) {
                v
            } else if let Ok(v) = row.get::<usize, i64>(1) {
                v as u64
            } else if let Ok(s) = row.get::<usize, String>(1) {
                match s.parse::<u64>() {
                    Ok(n) => n,
                    Err(_) => continue,
                }
            } else {
                continue;
            };

            res.insert(id, max_ts);
        }
        Ok(res)
    }

    async fn get_history_between(&self, inst_id: u64, start_ms: u64, end_ms: u64) -> Result<Vec<PolyMarketHistoryPo>, YuError> {
        let conn = self.provider.acquire()?;

        // Query by numeric instrument_id
        let sql = format!(
            "SELECT instrument_id, timestamp, price FROM polymarket_price_history WHERE instrument_id = {} AND timestamp >= {} AND timestamp <= {} ORDER BY timestamp DESC;",
            inst_id, start_ms, end_ms
        );

        let mut stmt = conn.prepare(sql.as_str())?;
        let mut rows = stmt.query([])?;
        let mut res: Vec<PolyMarketHistoryPo> = Vec::new();

        while let Some(row) = rows.next()? {
            // instrument_id may be stored as u64, i64 or string
            let instrument_id: u64 = if let Ok(v) = row.get::<usize, u64>(0) {
                v
            } else if let Ok(v) = row.get::<usize, i64>(0) {
                v as u64
            } else if let Ok(s) = row.get::<usize, String>(0) {
                s.parse::<u64>().unwrap_or(0)
            } else {
                0
            };

            // timestamp may be numeric or string
            let timestamp: u64 = if let Ok(v) = row.get::<usize, u64>(1) {
                v
            } else if let Ok(v) = row.get::<usize, i64>(1) {
                v as u64
            } else if let Ok(s) = row.get::<usize, String>(1) {
                s.parse::<u64>().unwrap_or(0)
            } else {
                0
            };

            // price as f64 (or parse from string/int)
            let price: f64 = if let Ok(v) = row.get::<usize, f64>(2) {
                v
            } else if let Ok(v) = row.get::<usize, i64>(2) {
                v as f64
            } else if let Ok(v) = row.get::<usize, i32>(2) {
                v as f64
            } else if let Ok(s) = row.get::<usize, String>(2) {
                s.parse::<f64>().unwrap_or(0.0)
            } else {
                0.0
            };

            res.push(PolyMarketHistoryPo {
                instrument_id,
                timestamp,
                price,
            });
        }

        Ok(res)
    }
}
