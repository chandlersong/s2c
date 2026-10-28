use crate::errors::YuError;
use crate::sync::client::po::binance::LocalBinanceInstrumentPo;
use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;

const FIND_LATEST_SPOT_KLINE_TIMESTAMP_SQL: &str = "
    SELECT MAX(h.candle_begin_time)
    FROM binance_instruments i
    LEFT JOIN binance_spot_kline_history h ON h.inst_id = i.id
    WHERE i.server_id = $1
";

const FIND_LATEST_SWAP_KLINE_TIMESTAMP_SQL: &str = "
    SELECT MAX(h.candle_begin_time)
    FROM binance_instruments i
    LEFT JOIN binance_swap_kline_history h ON h.inst_id = i.id
    WHERE i.server_id = $1
";

const FIND_LATEST_SWAP_FUNDING_RATE_TIMESTAMP_SQL: &str = "
    SELECT MAX(h.funding_time)
    FROM binance_instruments i
    LEFT JOIN binance_swap_funding_rate_history h ON h.inst_id = i.id
    WHERE i.server_id = $1
";

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait ClientBinanceRepositoryTrait: Send + Sync {
    async fn list_all_instruments(&self) -> Result<Vec<LocalBinanceInstrumentPo>, YuError>;

    async fn find_instrument_id_by_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError>;

    async fn find_latest_kline_timestamp_by_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError>;

    async fn find_latest_swap_kline_timestamp_by_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError>;

    async fn find_latest_swap_funding_rate_timestamp_by_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError>;

    async fn create_instrument(&self, instrument: LocalBinanceInstrumentPo) -> Result<u64, YuError>;
}

pub type ClientBinanceRepository = Arc<dyn ClientBinanceRepositoryTrait>;

pub struct ClientBinanceRepositoryImpl {
    pg_pool: PgPool,
}

impl ClientBinanceRepositoryImpl {
    pub fn from_pool(pg_pool: PgPool) -> ClientBinanceRepository {
        Arc::new(Self { pg_pool })
    }
}

#[async_trait]
impl ClientBinanceRepositoryTrait for ClientBinanceRepositoryImpl {
    async fn list_all_instruments(&self) -> Result<Vec<LocalBinanceInstrumentPo>, YuError> {
        Ok(sqlx::query_as::<_, LocalBinanceInstrumentPo>("SELECT * FROM binance_instruments")
            .fetch_all(&self.pg_pool)
            .await?)
    }

    async fn find_instrument_id_by_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError> {
        let server_id = i64::try_from(server_id).map_err(|_| YuError::new("Binance server instrument ID exceeds BIGINT range"))?;
        let id: Option<i64> = sqlx::query_scalar("SELECT id FROM binance_instruments WHERE server_id = $1")
            .bind(server_id)
            .fetch_optional(&self.pg_pool)
            .await?;
        id.map(|id| u64::try_from(id).map_err(|_| YuError::new("negative local Binance instrument ID")))
            .transpose()
    }

    async fn find_latest_kline_timestamp_by_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError> {
        self.find_latest_kline_timestamp(server_id, FIND_LATEST_SPOT_KLINE_TIMESTAMP_SQL).await
    }

    async fn find_latest_swap_kline_timestamp_by_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError> {
        self.find_latest_kline_timestamp(server_id, FIND_LATEST_SWAP_KLINE_TIMESTAMP_SQL).await
    }

    async fn find_latest_swap_funding_rate_timestamp_by_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError> {
        self.find_latest_history_timestamp(server_id, FIND_LATEST_SWAP_FUNDING_RATE_TIMESTAMP_SQL)
            .await
    }

    async fn create_instrument(&self, instrument: LocalBinanceInstrumentPo) -> Result<u64, YuError> {
        let id = i64::try_from(instrument.id).map_err(|_| YuError::new("local Binance instrument ID exceeds BIGINT range"))?;
        let server_id = i64::try_from(instrument.server_id).map_err(|_| YuError::new("Binance server instrument ID exceeds BIGINT range"))?;
        let on_board_time = instrument
            .on_board_time
            .map(i64::try_from)
            .transpose()
            .map_err(|_| YuError::new("Binance instrument listing time exceeds BIGINT range"))?;
        let inserted_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO binance_instruments (
                id, server_id, symbol, status, base_asset, quote_asset, quote_asset_precision,
                order_types, symbol_type, on_board_time
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
            ON CONFLICT (server_id) DO UPDATE SET server_id = EXCLUDED.server_id
            RETURNING id
            "#,
        )
        .bind(id)
        .bind(server_id)
        .bind(instrument.symbol)
        .bind(instrument.status)
        .bind(instrument.base_asset)
        .bind(instrument.quote_asset)
        .bind(instrument.quote_asset_precision)
        .bind(instrument.order_types)
        .bind(instrument.symbol_type)
        .bind(on_board_time)
        .fetch_one(&self.pg_pool)
        .await?;

        u64::try_from(inserted_id).map_err(|_| YuError::new("negative local Binance instrument ID"))
    }
}

impl ClientBinanceRepositoryImpl {
    async fn find_latest_kline_timestamp(&self, server_id: u64, query: &'static str) -> Result<Option<u64>, YuError> {
        self.find_latest_history_timestamp(server_id, query).await
    }

    async fn find_latest_history_timestamp(&self, server_id: u64, query: &'static str) -> Result<Option<u64>, YuError> {
        let server_id = i64::try_from(server_id).map_err(|_| YuError::new("Binance server instrument ID exceeds BIGINT range"))?;
        let timestamp: Option<chrono::DateTime<chrono::Utc>> =
            sqlx::query_scalar(query).bind(server_id).fetch_optional(&self.pg_pool).await?.flatten();
        timestamp
            .map(|timestamp| u64::try_from(timestamp.timestamp_millis()).map_err(|_| YuError::new("negative Binance history timestamp")))
            .transpose()
    }
}
