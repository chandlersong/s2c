use crate::duck_db::DuckDBDSProvider;
use crate::duck_db_tables::DuckTableTableChannel;
use crate::errors::YuError;
use crate::okx::duck_po::{InstrumentPo, OkxKlinePo};
use crate::okx::duckdb_tables::get_okx_kline_table;
use crate::okx::okx_consts::InstrumentType;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::oneshot;
use yue::query_message::{BatchInsertPayload, DataSourceProviderTrait, InsertPayload, QueryCommand};
use yue::tools::get_snow_flake_id_u64;

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait OkxInstrumentRepositoryTrait {
    async fn get_instrument_by_type(&self, inst_type: InstrumentType) -> Result<Vec<InstrumentPo>, YuError>;
    async fn get_instrument_by_identify(&self, inst_identify: &str) -> Result<InstrumentPo, YuError>;
    async fn get_instrument_by_type_live(&self, inst_type: InstrumentType) -> Result<Vec<InstrumentPo>, YuError>;

    async fn insert_instrument(&self, instrument: InstrumentPo) -> Result<(), YuError>;

    ///
    /// 根据instrument中的inst_id进行更新
    ///
    async fn update_instrument(&self, instrument: InstrumentPo) -> Result<(), YuError>;
}

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait OkxKlineRepositoryTrait {
    async fn instrument_max_timestamp(&self, inst_id: u64) -> Result<Option<u64>, YuError>;

    async fn max_timestamp_group_by_inst_id_before(&self, before: u64) -> Result<HashMap<u64, u64>, YuError>;

    async fn insert_history(&self, po: OkxKlinePo) -> Result<(), YuError>;

    async fn batch_insert(&self, po_vec: Vec<OkxKlinePo>) -> Result<(), YuError>;

    async fn find_kline_after(&self, inst_id: u64, ts: u64) -> Result<Vec<OkxKlinePo>, YuError>;
}

pub type OkxInstrumentRepository = Arc<dyn OkxInstrumentRepositoryTrait + Send + Sync>;
pub type OkxKlineRepository = Arc<dyn OkxKlineRepositoryTrait + Send + Sync>;

pub fn get_instrument_repo(provider: Option<DuckDBDSProvider>) -> OkxInstrumentRepository {
    let real_provider = provider.unwrap_or_default();
    Arc::new(OkxInstrumentRepositoryImpl { provider: real_provider })
}

pub fn get_default_kline_repo(provider: Option<DuckDBDSProvider>) -> OkxKlineRepository {
    let real_provider = provider.unwrap_or_default();
    Arc::new(OkxKlinePoRepositoryImpl {
        provider: real_provider,
        channel: get_okx_kline_table(),
    })
}

struct OkxInstrumentRepositoryImpl {
    provider: DuckDBDSProvider,
}

#[async_trait]
impl OkxInstrumentRepositoryTrait for OkxInstrumentRepositoryImpl {
    async fn get_instrument_by_type(&self, inst_type: InstrumentType) -> Result<Vec<InstrumentPo>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare("SELECT id, inst_identify, inst_type, inst_family, base_ccy, quote_ccy, settle_ccy, list_time, exp_time, tick_sz, lot_sz, min_sz, alias, state, inst_id_code, inst_category FROM OKX_INSTRUMENTS where inst_type = ?;")?;
        let rows = stmt.query([inst_type.as_str()])?;
        InstrumentPo::from_db_to_vec(rows)
    }

    async fn get_instrument_by_identify(&self, inst_id: &str) -> Result<InstrumentPo, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare("SELECT id, inst_identify, inst_type, inst_family, base_ccy, quote_ccy, settle_ccy, list_time, exp_time, tick_sz, lot_sz, min_sz, alias, state, inst_id_code, inst_category FROM OKX_INSTRUMENTS where id = ?;")?;
        let mut rows = stmt.query([inst_id])?;
        // Expect at most one row. Read the first row if present and convert it to InstrumentPo.
        if let Some(row) = rows.next()? {
            let po = InstrumentPo::try_from(row)?;
            Ok(po)
        } else {
            Err(YuError::new("instrument not found"))
        }
    }

    async fn get_instrument_by_type_live(&self, inst_type: InstrumentType) -> Result<Vec<InstrumentPo>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare("SELECT id, inst_identify, inst_type, inst_family, base_ccy, quote_ccy, settle_ccy, list_time, exp_time, tick_sz, lot_sz, min_sz, alias, state, inst_id_code, inst_category FROM OKX_INSTRUMENTS where inst_type = ? and state='live';")?;
        let rows = stmt.query([inst_type.as_str()])?;
        InstrumentPo::from_db_to_vec(rows)
    }

    async fn insert_instrument(&self, instrument: InstrumentPo) -> Result<(), YuError> {
        let conn = self.provider.acquire()?;

        // helpers
        let esc = |s: &str| s.replace('\'', "''");
        let q_str = |o: &Option<String>| match o {
            Some(v) => format!("'{}'", esc(v)),
            None => "NULL".to_string(),
        };

        let list_time = instrument.list_time.map(|v| format!("'{}'", v)).unwrap_or_else(|| "NULL".to_string());
        let exp_time = instrument.exp_time.map(|v| format!("'{}'", v)).unwrap_or_else(|| "NULL".to_string());
        let tick_sz = instrument.tick_sz.map(|f| f.to_string()).unwrap_or_else(|| "NULL".to_string());
        let lot_sz = instrument.lot_sz.map(|f| f.to_string()).unwrap_or_else(|| "NULL".to_string());
        let min_sz = instrument.min_sz.map(|f| f.to_string()).unwrap_or_else(|| "NULL".to_string());

        let insert_sql = format!(
            "INSERT INTO OKX_INSTRUMENTS(id, inst_identify, inst_type, inst_family, base_ccy, quote_ccy, settle_ccy, list_time, exp_time, tick_sz, lot_sz, min_sz, alias, state, inst_id_code, inst_category) VALUES ({},'{}','{}',{},'{}',{},{},{},{},{},{},{},{},{},{},{});",
            get_snow_flake_id_u64(),
            esc(&instrument.inst_identify),
            esc(&instrument.inst_type),
            q_str(&instrument.inst_family),
            esc(&instrument.base_ccy),
            q_str(&instrument.quote_ccy),
            q_str(&instrument.settle_ccy),
            list_time,
            exp_time,
            tick_sz,
            lot_sz,
            min_sz,
            q_str(&instrument.alias),
            q_str(&instrument.state),
            q_str(&instrument.inst_id_code),
            q_str(&instrument.inst_category)
        );

        conn.execute(insert_sql.as_str(), [])?;
        Ok(())
    }

    async fn update_instrument(&self, instrument: InstrumentPo) -> Result<(), YuError> {
        let conn = self.provider.acquire()?;

        let esc = |s: &str| s.replace('\'', "''");
        let q_str = |o: &Option<String>| match o {
            Some(v) => format!("'{}'", esc(v)),
            None => "NULL".to_string(),
        };
        let list_time = instrument.list_time.map(|v| format!("'{}'", v)).unwrap_or_else(|| "NULL".to_string());
        let exp_time = instrument.exp_time.map(|v| format!("'{}'", v)).unwrap_or_else(|| "NULL".to_string());
        let tick_sz = instrument.tick_sz.map(|f| f.to_string()).unwrap_or_else(|| "NULL".to_string());
        let lot_sz = instrument.lot_sz.map(|f| f.to_string()).unwrap_or_else(|| "NULL".to_string());
        let min_sz = instrument.min_sz.map(|f| f.to_string()).unwrap_or_else(|| "NULL".to_string());

        let update_sql = format!(
            "UPDATE OKX_INSTRUMENTS SET inst_identify='{}', inst_type='{}', inst_family={}, base_ccy='{}', quote_ccy={}, settle_ccy={}, list_time={}, exp_time={}, tick_sz={}, lot_sz={}, min_sz={}, alias={}, state={}, inst_id_code={}, inst_category={} WHERE id={};",
            esc(&instrument.inst_identify),
            esc(&instrument.inst_type),
            q_str(&instrument.inst_family),
            esc(&instrument.base_ccy),
            q_str(&instrument.quote_ccy),
            q_str(&instrument.settle_ccy),
            list_time,
            exp_time,
            tick_sz,
            lot_sz,
            min_sz,
            q_str(&instrument.alias),
            q_str(&instrument.state),
            q_str(&instrument.inst_id_code),
            q_str(&instrument.inst_category),
            instrument.id
        );

        conn.execute(update_sql.as_str(), [])?;
        Ok(())
    }
}

pub struct OkxKlinePoRepositoryImpl {
    provider: DuckDBDSProvider,
    channel: DuckTableTableChannel<OkxKlinePo>,
}

impl Default for OkxKlinePoRepositoryImpl {
    fn default() -> Self {
        Self {
            provider: Default::default(),
            channel: get_okx_kline_table(),
        }
    }
}

#[async_trait]
impl OkxKlineRepositoryTrait for OkxKlinePoRepositoryImpl {
    async fn instrument_max_timestamp(&self, inst_id: u64) -> Result<Option<u64>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare("SELECT max(timestamp) FROM OKX_KLINE where inst_id = ?;")?;
        let mut rows = stmt.query([inst_id])?;
        if let Some(row) = rows.next()? {
            let max_timestamp: Option<u64> = row.get(0)?;
            Ok(max_timestamp)
        } else {
            Ok(None)
        }
    }

    async fn max_timestamp_group_by_inst_id_before(&self, before: u64) -> Result<HashMap<u64, u64>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare("SELECT inst_id, max(timestamp) FROM OKX_KLINE WHERE timestamp < ? GROUP BY inst_id;")?;
        let mut rows = stmt.query([before])?;
        let mut res: HashMap<u64, u64> = HashMap::new();
        while let Some(row) = rows.next()? {
            let inst_id: u64 = row.get(0)?;
            let max_timestamp: u64 = row.get(1)?;
            res.insert(inst_id, max_timestamp);
        }
        Ok(res)
    }

    async fn insert_history(&self, po: OkxKlinePo) -> Result<(), YuError> {
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

    async fn batch_insert(&self, po_vec: Vec<OkxKlinePo>) -> Result<(), YuError> {
        let (tx, rx) = oneshot::channel();
        let command = QueryCommand::BatchInsert(BatchInsertPayload::new(po_vec, tx));
        self.channel
            .send(command)
            .await
            .map_err(|e| YuError::new(&format!("Failed to send command: {}", e)))?;
        match rx.await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(YuError::new(&format!("Failed to receive command: {}", e))),
            Err(e) => Err(YuError::new(&format!("Failed to receive command: {}", e))),
        }
    }

    async fn find_kline_after(&self, inst_id: u64, ts: u64) -> Result<Vec<OkxKlinePo>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare("SELECT id, inst_id, timestamp, open, high, low, close, volume, volCcy, volCcyQuote, confirm FROM OKX_KLINE WHERE inst_id = ? AND timestamp > ? ORDER BY timestamp ASC;")?;
        let mut rows = stmt.query([inst_id, ts])?;
        let mut res: Vec<OkxKlinePo> = Vec::new();
        while let Some(row) = rows.next()? {
            let id: u64 = row.get(0)?;
            let inst_id_db: u64 = row.get(1)?;
            let ts_db: u64 = row.get(2)?;
            let open: f64 = row.get(3)?;
            let high: f64 = row.get(4)?;
            let low: f64 = row.get(5)?;
            let close: f64 = row.get(6)?;
            let vol: f64 = row.get(7)?;
            let vol_ccy: f64 = row.get(8)?;
            let vol_ccy_quote: f64 = row.get(9)?;
            let confirm_i: i32 = row.get(10)?;
            let confirm: u8 = confirm_i as u8;

            res.push(OkxKlinePo {
                id,
                inst_id: inst_id_db,
                ts: ts_db,
                open,
                high,
                low,
                close,
                vol,
                vol_ccy,
                vol_ccy_quote,
                confirm,
            });
        }
        Ok(res)
    }
}
