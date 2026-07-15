use crate::duck_db::DuckDBDSProvider;
use crate::errors::YuError;
use crate::okx::duck_po::InstrumentPo;
use crate::okx::okx_consts::InstrumentType;
use async_trait::async_trait;
use std::sync::Arc;
use yue::query_message::DataSourceProviderTrait;

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait OkxInstrumentRepositoryTrait {
    async fn get_instrument_by_type(&self, inst_type: InstrumentType) -> Result<Vec<InstrumentPo>, YuError>;
    async fn get_instrument_by_type_live(&self, inst_type: InstrumentType) -> Result<Vec<InstrumentPo>, YuError>;

    async fn insert_instrument(&self, instrument: InstrumentPo) -> Result<(), YuError>;

    ///
    /// 根据instrument中的inst_id进行更新
    ///
    async fn update_instrument(&self, instrument: InstrumentPo) -> Result<(), YuError>;
}

pub type OkxInstrumentRepository = Arc<dyn OkxInstrumentRepositoryTrait + Send + Sync>;

pub fn get_instrument_repo(provider: Option<DuckDBDSProvider>) -> OkxInstrumentRepository {
    let real_provider = provider.unwrap_or_default();
    Arc::new(OkxInstrumentRepositoryImpl { provider: real_provider })
}

struct OkxInstrumentRepositoryImpl {
    provider: DuckDBDSProvider,
}

#[async_trait]
impl OkxInstrumentRepositoryTrait for OkxInstrumentRepositoryImpl {
    async fn get_instrument_by_type(&self, inst_type: InstrumentType) -> Result<Vec<InstrumentPo>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare("SELECT instId, instType, instFamily, baseCcy, quoteCcy, settleCcy, listTime, expTime, tickSz, lotSz, minSz, alias, state, instIdCode, instCategory FROM OKX_INSTRUMENTS where instType = ?;")?;
        let rows = stmt.query([inst_type.as_str()])?;
        InstrumentPo::from_db_to_vec(rows)
    }

    async fn get_instrument_by_type_live(&self, inst_type: InstrumentType) -> Result<Vec<InstrumentPo>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare("SELECT instId, instType, instFamily, baseCcy, quoteCcy, settleCcy, listTime, expTime, tickSz, lotSz, minSz, alias, state, instIdCode, instCategory FROM OKX_INSTRUMENTS where instType = ? and state='live';")?;
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

        let tick_sz = instrument.tick_sz.map(|f| f.to_string()).unwrap_or_else(|| "NULL".to_string());
        let lot_sz = instrument.lot_sz.map(|f| f.to_string()).unwrap_or_else(|| "NULL".to_string());
        let min_sz = instrument.min_sz.map(|f| f.to_string()).unwrap_or_else(|| "NULL".to_string());

        let insert_sql = format!(
            "INSERT INTO OKX_INSTRUMENTS(instId, instType, instFamily, baseCcy, quoteCcy, settleCcy, listTime, expTime, tickSz, lotSz, minSz, alias, state, instIdCode, instCategory) VALUES ('{}','{}',{},'{}',{},{},{},{},{},{},{},{},{} ,{},{});",
            esc(&instrument.inst_id),
            esc(&instrument.inst_type),
            q_str(&instrument.inst_family),
            esc(&instrument.base_ccy),
            q_str(&instrument.quote_ccy),
            q_str(&instrument.settle_ccy),
            q_str(&instrument.list_time),
            q_str(&instrument.exp_time),
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

        let tick_sz = instrument.tick_sz.map(|f| f.to_string()).unwrap_or_else(|| "NULL".to_string());
        let lot_sz = instrument.lot_sz.map(|f| f.to_string()).unwrap_or_else(|| "NULL".to_string());
        let min_sz = instrument.min_sz.map(|f| f.to_string()).unwrap_or_else(|| "NULL".to_string());

        let update_sql = format!(
            "UPDATE OKX_INSTRUMENTS SET instType='{}', instFamily={}, baseCcy='{}', quoteCcy={}, settleCcy={}, listTime={}, expTime={}, tickSz={}, lotSz={}, minSz={}, alias={}, state={}, instIdCode={}, instCategory={} WHERE instId='{}';",
            esc(&instrument.inst_type),
            q_str(&instrument.inst_family),
            esc(&instrument.base_ccy),
            q_str(&instrument.quote_ccy),
            q_str(&instrument.settle_ccy),
            q_str(&instrument.list_time),
            q_str(&instrument.exp_time),
            tick_sz,
            lot_sz,
            min_sz,
            q_str(&instrument.alias),
            q_str(&instrument.state),
            q_str(&instrument.inst_id_code),
            q_str(&instrument.inst_category),
            esc(&instrument.inst_id)
        );

        conn.execute(update_sql.as_str(), [])?;
        Ok(())
    }
}
