use crate::errors::YuError;
use crate::sync::client::po::polymarket::LocalPolyMarketInstrumentPo;
use async_trait::async_trait;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait ClientPolyMarketRepositoryTrait {
    async fn list_all_instrument(&self) -> Result<Vec<LocalPolyMarketInstrumentPo>, YuError>;

    //返回server id+timestamp
    async fn list_instrument_timestamps(&self) -> Result<HashMap<u64, u64>, YuError>;

    //返回一个mapping。key是每个的server_id,value是LocalPolyMarketInstrumentPo
    async fn server_id_instrument_dictionary(&self) -> Result<HashMap<u64, LocalPolyMarketInstrumentPo>, YuError> {
        let all_instruments = self.list_all_instrument().await?;

        let map: HashMap<u64, LocalPolyMarketInstrumentPo> = all_instruments.into_iter().map(|inst| (inst.server_id, inst)).collect();

        Ok(map)
    }
    /// Insert a new asset info into polymarket_assert_info table.
    async fn create_instruments(&self, po: LocalPolyMarketInstrumentPo) -> Result<(), YuError>;
}

pub type ClientPolyMarketRepository = Arc<dyn ClientPolyMarketRepositoryTrait + Send + Sync>;

pub struct ClientPolyMarketRepositoryImpl {
    pg_pool: PgPool,
}

impl ClientPolyMarketRepositoryImpl {
    pub fn from_pool(pg_pool: PgPool) -> ClientPolyMarketRepository {
        Arc::new(Self { pg_pool })
    }
}

#[async_trait]
impl ClientPolyMarketRepositoryTrait for ClientPolyMarketRepositoryImpl {
    async fn list_all_instrument(&self) -> Result<Vec<LocalPolyMarketInstrumentPo>, YuError> {
        Ok(
            sqlx::query_as::<_, LocalPolyMarketInstrumentPo>(r#"select * from polymarket_instruments"#)
                .fetch_all(&self.pg_pool)
                .await?,
        )
    }

    async fn list_instrument_timestamps(&self) -> Result<HashMap<u64, u64>, YuError> {
        let sql = r#"
            SELECT pi.server_id::bigint AS server_id, (EXTRACT(EPOCH FROM max(pph.timestamp)) * 1000)::bigint AS max_ts
            FROM polymarket_instruments pi
            JOIN polymarket_price_history pph ON pph.instrument_id = pi.id
            GROUP BY pi.server_id
        "#;
        let rows: Vec<(i64, i64)> = sqlx::query_as(sql).fetch_all(&self.pg_pool).await?;

        let map: HashMap<u64, u64> = rows.into_iter().map(|(k, v)| (k as u64, v as u64)).collect();
        Ok(map)
    }

    async fn create_instruments(&self, po: LocalPolyMarketInstrumentPo) -> Result<(), YuError> {
        let sql = r#"INSERT INTO polymarket_instruments (id, server_id, series_id, series_slug, event_id, event_slug, market_id, market_slug, assert_id, assert_slug, start_ms, end_ms)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) ON CONFLICT (id) DO NOTHING"#;

        sqlx::query(sql)
            .bind(po.id as i64)
            .bind(po.server_id as i64)
            .bind(&po.series_id)
            .bind(&po.series_slug)
            .bind(&po.event_id)
            .bind(&po.event_slug)
            .bind(&po.market_id)
            .bind(&po.market_slug)
            .bind(&po.assert_id)
            .bind(&po.assert_slug)
            .bind(po.start_ms as i64)
            .bind(po.end_ms as i64)
            .execute(&self.pg_pool)
            .await?;

        Ok(())
    }
}
