use crate::errors::YuError;
use crate::postgresql_db::get_sync_client_pg_pool_sync;
use crate::sync::client::po::polymarket::LocalPolyMarketInstrumentPo;
use async_trait::async_trait;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait ClientPolyMarketRepositoryTrait {
    async fn list_all_instrument(&self) -> Result<Vec<LocalPolyMarketInstrumentPo>, YuError>;

    async fn get_live_instruments(&self, at_ms: u64) -> Result<Vec<LocalPolyMarketInstrumentPo>, YuError>;

    //返回server id+数据库中timestamp
    async fn list_instrument_timestamps(&self) -> Result<HashMap<u64, u64>, YuError>;

    //返回一个mapping。key是每个的server_id,value是LocalPolyMarketInstrumentPo
    async fn server_id_instrument_dictionary(&self) -> Result<HashMap<u64, LocalPolyMarketInstrumentPo>, YuError> {
        let all_instruments = self.list_all_instrument().await?;

        let map: HashMap<u64, LocalPolyMarketInstrumentPo> = all_instruments.into_iter().map(|inst| (inst.server_id, inst)).collect();

        Ok(map)
    }
    /// Insert a new asset info into polymarket_assert_info table.
    async fn create_instruments(&self, po: LocalPolyMarketInstrumentPo) -> Result<u64, YuError>;

    async fn update_instrument(&self, po: LocalPolyMarketInstrumentPo) -> Result<(), YuError>;

    async fn find_instrument_by_server_id(&self, server_id: u64) -> Result<Option<LocalPolyMarketInstrumentPo>, YuError>;

    //通过server id 返回数据库中最最新的timestamp
    async fn get_lastest_timestamps_by_server_id(&self, server_id: u64) -> Result<u64, YuError>;
}

pub type ClientPolyMarketRepository = Arc<dyn ClientPolyMarketRepositoryTrait + Send + Sync>;

pub fn default_client_polymarket_repository() -> ClientPolyMarketRepository {
    match get_sync_client_pg_pool_sync() {
        Ok(pool) => ClientPolyMarketRepositoryImpl::from_pool(pool),
        Err(e) => {
            panic!("initial pg pool fail,{}", e)
        }
    }
}

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

    async fn get_live_instruments(&self, at_ms: u64) -> Result<Vec<LocalPolyMarketInstrumentPo>, YuError> {
        Ok(
            sqlx::query_as::<_, LocalPolyMarketInstrumentPo>("SELECT * FROM polymarket_instruments WHERE end_ms > $1")
                .bind(i64::try_from(at_ms).map_err(|_| YuError::new("live instrument cutoff exceeds BIGINT range"))?)
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

    async fn create_instruments(&self, po: LocalPolyMarketInstrumentPo) -> Result<u64, YuError> {
        let sql = r#"INSERT INTO polymarket_instruments (id, server_id, series_id, series_slug, event_id, event_slug, market_id, market_slug, assert_id, assert_slug, start_ms, end_ms)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
            ON CONFLICT (assert_id) DO UPDATE SET
                server_id = EXCLUDED.server_id,
                series_id = EXCLUDED.series_id,
                series_slug = EXCLUDED.series_slug,
                event_id = EXCLUDED.event_id,
                event_slug = EXCLUDED.event_slug,
                market_id = EXCLUDED.market_id,
                market_slug = EXCLUDED.market_slug,
                assert_slug = EXCLUDED.assert_slug,
                start_ms = EXCLUDED.start_ms,
                end_ms = EXCLUDED.end_ms
            RETURNING id"#;

        let id = i64::try_from(po.id).map_err(|_| YuError::new("local Polymarket instrument ID exceeds BIGINT range"))?;
        let server_id = i64::try_from(po.server_id).map_err(|_| YuError::new("Polymarket server instrument ID exceeds BIGINT range"))?;
        let start_ms = i64::try_from(po.start_ms).map_err(|_| YuError::new("Polymarket start time exceeds BIGINT range"))?;
        let end_ms = i64::try_from(po.end_ms).map_err(|_| YuError::new("Polymarket end time exceeds BIGINT range"))?;
        let inserted_id: i64 = sqlx::query_scalar(sql)
            .bind(id)
            .bind(server_id)
            .bind(&po.series_id)
            .bind(&po.series_slug)
            .bind(&po.event_id)
            .bind(&po.event_slug)
            .bind(&po.market_id)
            .bind(&po.market_slug)
            .bind(&po.assert_id)
            .bind(&po.assert_slug)
            .bind(start_ms)
            .bind(end_ms)
            .fetch_one(&self.pg_pool)
            .await?;

        u64::try_from(inserted_id).map_err(|_| YuError::new("negative local Polymarket instrument ID"))
    }

    async fn update_instrument(&self, po: LocalPolyMarketInstrumentPo) -> Result<(), YuError> {
        let id = i64::try_from(po.id).map_err(|_| YuError::new("local Polymarket instrument ID exceeds BIGINT range"))?;
        let server_id = i64::try_from(po.server_id).map_err(|_| YuError::new("Polymarket server instrument ID exceeds BIGINT range"))?;
        let start_ms = i64::try_from(po.start_ms).map_err(|_| YuError::new("Polymarket start time exceeds BIGINT range"))?;
        let end_ms = i64::try_from(po.end_ms).map_err(|_| YuError::new("Polymarket end time exceeds BIGINT range"))?;
        let result = sqlx::query(
            r#"
            UPDATE polymarket_instruments
            SET series_id = $1, series_slug = $2, event_id = $3, event_slug = $4, market_id = $5,
                market_slug = $6, assert_id = $7, assert_slug = $8, start_ms = $9, end_ms = $10
            WHERE server_id = $11 AND id = $12
            "#,
        )
        .bind(po.series_id)
        .bind(po.series_slug)
        .bind(po.event_id)
        .bind(po.event_slug)
        .bind(po.market_id)
        .bind(po.market_slug)
        .bind(po.assert_id)
        .bind(po.assert_slug)
        .bind(start_ms)
        .bind(end_ms)
        .bind(server_id)
        .bind(id)
        .execute(&self.pg_pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(YuError::new("Polymarket instrument disappeared during reconciliation"));
        }
        Ok(())
    }

    async fn find_instrument_by_server_id(&self, server_id: u64) -> Result<Option<LocalPolyMarketInstrumentPo>, YuError> {
        Ok(
            sqlx::query_as::<_, LocalPolyMarketInstrumentPo>("SELECT * FROM polymarket_instruments WHERE server_id = $1")
                .bind(server_id as i64)
                .fetch_optional(&self.pg_pool)
                .await?,
        )
    }

    async fn get_lastest_timestamps_by_server_id(&self, server_id: u64) -> Result<u64, YuError> {
        let timestamp: Option<i64> = sqlx::query_scalar(
            r#"
            SELECT (EXTRACT(EPOCH FROM MAX(pph.timestamp)) * 1000)::bigint
            FROM polymarket_instruments pi
            LEFT JOIN polymarket_price_history pph ON pph.instrument_id = pi.id
            WHERE pi.server_id = $1
            "#,
        )
        .bind(server_id as i64)
        .fetch_one(&self.pg_pool)
        .await?;

        match timestamp {
            Some(timestamp) => u64::try_from(timestamp).map_err(|_| YuError::new("negative Polymarket history timestamp")),
            None => Ok(0),
        }
    }
}
