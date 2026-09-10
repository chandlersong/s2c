use crate::errors::YuError;
use crate::sync::client::po::okx::LocalOkxInstrumentPo;
use async_trait::async_trait;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait ClientOkxRepositoryTrait {
    async fn list_all_instrument(&self) -> Result<Vec<LocalOkxInstrumentPo>, YuError>;

    // 返回一个mapping。key是每个的inst_id,value是LocalOkxInstrumentPo
    async fn server_id_instrument_dictionary(&self) -> Result<HashMap<u64, LocalOkxInstrumentPo>, YuError> {
        let all_instruments = self.list_all_instrument().await?;
        let map: HashMap<u64, LocalOkxInstrumentPo> = all_instruments.into_iter().map(|inst| (inst.server_id, inst)).collect();
        Ok(map)
    }

    //返回server id+数据库中timestamp
    async fn list_instrument_timestamps(&self) -> Result<HashMap<u64, u64>, YuError>;

    /// Insert a new okx instrument into okx_instruments table.
    async fn create_instruments(&self, po: LocalOkxInstrumentPo) -> Result<(), YuError>;
}

pub type ClientOkxRepository = Arc<dyn ClientOkxRepositoryTrait + Send + Sync>;

pub struct ClientOkxRepositoryImpl {
    pg_pool: PgPool,
}

impl ClientOkxRepositoryImpl {
    pub fn from_pool(pg_pool: PgPool) -> ClientOkxRepository {
        Arc::new(Self { pg_pool })
    }
}

#[async_trait]
impl ClientOkxRepositoryTrait for ClientOkxRepositoryImpl {
    async fn list_all_instrument(&self) -> Result<Vec<LocalOkxInstrumentPo>, YuError> {
        Ok(sqlx::query_as::<_, LocalOkxInstrumentPo>(r#"select * from okx_instruments"#)
            .fetch_all(&self.pg_pool)
            .await?)
    }

    async fn list_instrument_timestamps(&self) -> Result<HashMap<u64, u64>, YuError> {
        let sql = r#"
            SELECT pi.server_id::bigint AS server_id, (EXTRACT(EPOCH FROM max(pph.candle_begin_time)) * 1000)::bigint AS max_ts
            FROM okx_instruments pi
            JOIN okx_kline_history pph ON pph.instrument_id = pi.id
            GROUP BY pi.server_id
        "#;
        let rows: Vec<(i64, i64)> = sqlx::query_as(sql).fetch_all(&self.pg_pool).await?;

        let map: HashMap<u64, u64> = rows.into_iter().map(|(k, v)| (k as u64, v as u64)).collect();
        Ok(map)
    }

    async fn create_instruments(&self, po: LocalOkxInstrumentPo) -> Result<(), YuError> {
        let sql = r#"INSERT INTO okx_instruments (id, server_id, inst_identify, inst_type, inst_family, base_ccy, quote_ccy, settle_ccy, list_time, exp_time, tick_sz, lot_sz, min_sz, alias, state, inst_id_code, inst_category)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17) ON CONFLICT (id) DO NOTHING"#;

        sqlx::query(sql)
            .bind(po.id as i64)
            .bind(po.server_id as i64)
            .bind(&po.inst_identify)
            .bind(&po.inst_type)
            .bind(&po.inst_family)
            .bind(&po.base_ccy)
            .bind(&po.quote_ccy)
            .bind(&po.settle_ccy)
            .bind(po.list_time.map(|v| v as i64))
            .bind(po.exp_time.map(|v| v as i64))
            .bind(po.tick_sz)
            .bind(po.lot_sz)
            .bind(po.min_sz)
            .bind(&po.alias)
            .bind(&po.state)
            .bind(&po.inst_id_code)
            .bind(&po.inst_category)
            .execute(&self.pg_pool)
            .await?;

        Ok(())
    }
}
