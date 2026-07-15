use crate::duck_db::DuckDBDSProvider;
use crate::errors::YuError;
use crate::okx::duck_po::InstrumentPo;
use async_trait::async_trait;
use std::sync::Arc;
use yue::query_message::DataSourceProviderTrait;

#[async_trait]
pub trait OkxInstrumentRepositoryTrait {
    async fn get_instrument_by_type(&self, inst_type: &str) -> Result<Vec<InstrumentPo>, YuError>;

    async fn insert_instrument(&self, instrument: InstrumentPo) -> Result<(), YuError>;

    ///
    /// 根据instrument中的inst_id进行更新
    ///
    async fn update_instrument(&self, instrument: InstrumentPo) -> Result<(), YuError>;
}

pub type OkxInstrumentRepository = Arc<dyn OkxInstrumentRepositoryTrait + Send + Sync>;

struct OkxInstrumentRepositoryImpl {
    provider: DuckDBDSProvider,
}

#[async_trait]
impl OkxInstrumentRepositoryTrait for OkxInstrumentRepositoryImpl {
    async fn get_instrument_by_type(&self, inst_type: &str) -> Result<Vec<InstrumentPo>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare("SELECT instId, instType, instFamily, baseCcy, quoteCcy, settleCcy, listTime, expTime, tickSz, lotSz, minSz, alias, state, instIdCode, instCategory FROM OKX_INSTRUMENTS where instType = ?;")?;
        let mut rows = stmt.query([inst_type])?;
        let mut res = Vec::<InstrumentPo>::new();
        while let Some(row) = rows.next()? {
            let inst_id: String = row.get::<usize, String>(0)?;
            let inst_type_v: String = row.get::<usize, String>(1)?;
            let inst_family: Option<String> = row.get::<usize, Option<String>>(2)?;
            let base_ccy: String = row.get::<usize, String>(3)?;
            let quote_ccy: Option<String> = row.get::<usize, Option<String>>(4)?;
            let settle_ccy: Option<String> = row.get::<usize, Option<String>>(5)?;
            let list_time: Option<String> = row.get::<usize, Option<String>>(6)?;
            let exp_time: Option<String> = row.get::<usize, Option<String>>(7)?;
            let tick_sz: Option<f64> = row.get::<usize, Option<f64>>(8)?;
            let lot_sz: Option<f64> = row.get::<usize, Option<f64>>(9)?;
            let min_sz: Option<f64> = row.get::<usize, Option<f64>>(10)?;
            let alias: Option<String> = row.get::<usize, Option<String>>(11)?;
            let state: Option<String> = row.get::<usize, Option<String>>(12)?;
            let inst_id_code: Option<String> = row.get::<usize, Option<String>>(13)?;
            let inst_category: Option<String> = row.get::<usize, Option<String>>(14)?;

            let po = InstrumentPo {
                inst_id,
                inst_type: inst_type_v,
                inst_family,
                base_ccy,
                quote_ccy,
                settle_ccy,
                list_time,
                exp_time,
                tick_sz,
                lot_sz,
                min_sz,
                alias,
                state,
                inst_id_code,
                inst_category,
            };

            res.push(po);
        }
        Ok(res)
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
