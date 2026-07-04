use crate::errors::YuError;
use crate::sync::client::po::LocalPolyMarketAssetInfoPo;
use async_trait::async_trait;
use std::collections::HashMap;
use sqlx_postgres::PgPool;
use std::sync::Arc;

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait ClientPolyMarketRepositoryTrait {
    async fn list_all_assets(&self) -> Result<Vec<LocalPolyMarketAssetInfoPo>, YuError>;

    async fn list_assets_timestamp(&self) -> Result<HashMap<String, u64>, YuError>;

    /// Insert a new asset info into polymarket_assert_info table.
    async fn create_asset(&self, po: LocalPolyMarketAssetInfoPo) -> Result<(), YuError>;
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
    async fn list_all_assets(&self) -> Result<Vec<LocalPolyMarketAssetInfoPo>, YuError> {
        Ok(sqlx::query_as(r#"select * from polymarket_assert_info"#).fetch_all(&self.pg_pool).await?)
    }

    async fn list_assets_timestamp(&self) -> Result<HashMap<String, u64>, YuError> {
        let sql = "SELECT asset_id, max(timestamp) as max_ts FROM polymarket_price_history GROUP BY asset_id";
        let rows: Vec<(String, i64)> = sqlx::query_as(sql).fetch_all(&self.pg_pool).await?;

        let map: HashMap<String, u64> = rows.into_iter().map(|(k, v)| (k, v as u64)).collect();
        Ok(map)
    }

    async fn create_asset(&self, po: LocalPolyMarketAssetInfoPo) -> Result<(), YuError> {
        let sql = r#"INSERT INTO polymarket_assert_info (series_id, series_slug, event_id, event_slug, market_id, market_slug, assert_id, assert_slug)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (assert_id) DO NOTHING"#;

        sqlx::query(sql)
            .bind(&po.series_id)
            .bind(&po.series_slug)
            .bind(&po.event_id)
            .bind(&po.event_slug)
            .bind(&po.market_id)
            .bind(&po.market_slug)
            .bind(&po.assert_id)
            .bind(&po.assert_slug)
            .execute(&self.pg_pool)
            .await?;

        Ok(())
    }
}

