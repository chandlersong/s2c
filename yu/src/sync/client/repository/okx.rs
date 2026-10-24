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

    async fn find_instrument_id_by_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError>;

    // 返回一个mapping。key是每个的inst_id,value是LocalOkxInstrumentPo
    async fn server_id_instrument_dictionary(&self) -> Result<HashMap<u64, LocalOkxInstrumentPo>, YuError> {
        let all_instruments = self.list_all_instrument().await?;
        let map: HashMap<u64, LocalOkxInstrumentPo> = all_instruments.into_iter().map(|inst| (inst.server_id, inst)).collect();
        Ok(map)
    }

    //返回server id+数据库中timestamp
    async fn list_instrument_timestamps(&self) -> Result<HashMap<u64, u64>, YuError>;

    async fn find_latest_kline_timestamp_by_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError>;

    // 返回 server id -> latest acquire_ts (ms) 在 okx_option_summary 表中
    async fn list_option_summary_timestamps(&self) -> Result<HashMap<u64, u64>, YuError>;

    /// Insert a new okx instrument into okx_instruments table.
    async fn create_instruments(&self, po: LocalOkxInstrumentPo) -> Result<u64, YuError>;
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

    async fn find_instrument_id_by_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError> {
        let server_id = i64::try_from(server_id).map_err(|_| YuError::new("OKX server instrument ID exceeds BIGINT range"))?;
        let id: Option<i64> = sqlx::query_scalar("SELECT id FROM okx_instruments WHERE server_id = $1")
            .bind(server_id)
            .fetch_optional(&self.pg_pool)
            .await?;
        id.map(|id| u64::try_from(id).map_err(|_| YuError::new("negative local OKX instrument ID")))
            .transpose()
    }

    async fn list_instrument_timestamps(&self) -> Result<HashMap<u64, u64>, YuError> {
        let sql = r#"
            SELECT pi.server_id::bigint AS server_id, (EXTRACT(EPOCH FROM max(pph.timestamp)) * 1000)::bigint AS max_ts
            FROM okx_instruments pi
            JOIN okx_kline_history pph ON pph.instrument_id = pi.id
            GROUP BY pi.server_id
        "#;
        let rows: Vec<(i64, i64)> = sqlx::query_as(sql).fetch_all(&self.pg_pool).await?;

        let map: HashMap<u64, u64> = rows.into_iter().map(|(k, v)| (k as u64, v as u64)).collect();
        Ok(map)
    }

    async fn find_latest_kline_timestamp_by_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError> {
        let server_id = i64::try_from(server_id).map_err(|_| YuError::new("OKX server instrument ID exceeds BIGINT range"))?;
        let timestamp: Option<i64> = sqlx::query_scalar(
            r#"
            SELECT (EXTRACT(EPOCH FROM max(h.timestamp)) * 1000)::bigint
            FROM okx_instruments i
            LEFT JOIN okx_kline_history h ON h.instrument_id = i.id
            WHERE i.server_id = $1
            GROUP BY i.server_id
            "#,
        )
        .bind(server_id)
        .fetch_optional(&self.pg_pool)
        .await?
        .flatten();

        timestamp
            .map(|timestamp| u64::try_from(timestamp).map_err(|_| YuError::new("negative OKX kline timestamp")))
            .transpose()
    }

    async fn list_option_summary_timestamps(&self) -> Result<HashMap<u64, u64>, YuError> {
        let sql = r#"
            SELECT pi.server_id::bigint AS server_id, (EXTRACT(EPOCH FROM max(oac.acquire_ts)) * 1000)::bigint AS max_ts
            FROM okx_instruments pi
            JOIN okx_option_summary oac ON oac.instrument_id = pi.id
            GROUP BY pi.server_id
        "#;
        let rows: Vec<(i64, i64)> = sqlx::query_as(sql).fetch_all(&self.pg_pool).await?;
        let map: HashMap<u64, u64> = rows.into_iter().map(|(k, v)| (k as u64, v as u64)).collect();
        Ok(map)
    }

    async fn create_instruments(&self, po: LocalOkxInstrumentPo) -> Result<u64, YuError> {
        let sql = r#"INSERT INTO okx_instruments (id, server_id, inst_identify, inst_type, inst_family, base_ccy, quote_ccy, settle_ccy, list_time, exp_time, tick_sz, lot_sz, min_sz, alias, state, inst_id_code, inst_category)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17) ON CONFLICT (id) DO NOTHING
            RETURNING id"#;

        let id = i64::try_from(po.id).map_err(|_| YuError::new("local OKX instrument ID exceeds BIGINT range"))?;
        let server_id = i64::try_from(po.server_id).map_err(|_| YuError::new("OKX server instrument ID exceeds BIGINT range"))?;
        let inserted_id: i64 = sqlx::query_scalar(sql)
            .bind(id)
            .bind(server_id)
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
            .fetch_one(&self.pg_pool)
            .await?;

        u64::try_from(inserted_id).map_err(|_| YuError::new("negative local OKX instrument ID"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 测试目的：验证超出 PostgreSQL BIGINT 范围的服务端 instrument ID 会在查询前被拒绝。
    // 准备数据/过程：使用不建立真实连接的 PgPool，并传入 u64::MAX 调用最新 Kline 时间戳查询。
    // 预期结果：方法返回范围错误，不尝试连接数据库。
    #[tokio::test]
    async fn rejects_server_id_outside_bigint_range() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://user:password@localhost/database")
            .unwrap();
        let repository = ClientOkxRepositoryImpl { pg_pool: pool };

        let result = repository.find_latest_kline_timestamp_by_server_id(u64::MAX).await;

        assert_eq!(
            result.unwrap_err().to_string(),
            "Custom error: OKX server instrument ID exceeds BIGINT range"
        );
    }
}
