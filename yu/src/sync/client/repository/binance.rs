use crate::errors::YuError;
use crate::sync::client::po::binance::{
    BinanceInstrumentType, LocalBinanceInstrumentPo, LocalBinanceOptionInstrumentPo, LocalBinanceOptionSymbolFilterPo,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use std::sync::Arc;

const BINANCE_OPTION_INSTRUMENT_COLUMNS: &str = r#"
    SELECT id, server_id, (EXTRACT(EPOCH FROM expiry_date)::NUMERIC * 1000)::BIGINT AS expiry_date_ms,
        symbol, side, strike_price::TEXT AS strike_price, underlying, unit,
        liquidation_fee_rate::TEXT AS liquidation_fee_rate, min_qty::TEXT AS min_qty, max_qty::TEXT AS max_qty,
        initial_margin::TEXT AS initial_margin, maintenance_margin::TEXT AS maintenance_margin,
        min_initial_margin::TEXT AS min_initial_margin, min_maintenance_margin::TEXT AS min_maintenance_margin,
        price_scale, quantity_scale, quote_asset, contract_type, underlying_type, naked_sell, status, filters
    FROM binance_option_instruments
"#;

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

const FIND_LATEST_SPOT_TRADE_CURSOR_SQL: &str = "
    SELECT h.event_time, h.trade_id
    FROM binance_instruments i
    JOIN binance_spot_trade_history h ON h.inst_id = i.id
    WHERE i.server_id = $1
    ORDER BY h.event_time DESC, h.trade_id DESC
    LIMIT 1
";

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait ClientBinanceRepositoryTrait: Send + Sync {
    async fn list_all_instruments(&self, instrument_type: BinanceInstrumentType) -> Result<Vec<LocalBinanceInstrumentPo>, YuError>;

    async fn find_instrument_by_server_id(&self, server_id: u64) -> Result<Option<LocalBinanceInstrumentPo>, YuError>;

    async fn find_instrument_id_by_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError>;

    async fn find_latest_kline_timestamp_by_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError>;

    async fn find_latest_swap_kline_timestamp_by_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError>;

    async fn find_latest_swap_funding_rate_timestamp_by_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError>;

    async fn find_latest_spot_trade_cursor_by_server_id(&self, server_id: u64) -> Result<Option<(u64, i64)>, YuError>;

    async fn create_instrument(&self, instrument: LocalBinanceInstrumentPo) -> Result<u64, YuError>;

    async fn update_instrument(&self, instrument: LocalBinanceInstrumentPo) -> Result<(), YuError>;
}

pub type ClientBinanceRepository = Arc<dyn ClientBinanceRepositoryTrait>;

pub struct ClientBinanceRepositoryImpl {
    pg_pool: PgPool,
}

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait ClientBinanceOptionInstrumentRepositoryTrait: Send + Sync {
    async fn list_all_instruments(&self) -> Result<Vec<LocalBinanceOptionInstrumentPo>, YuError>;

    async fn find_instrument_by_server_id(&self, server_id: u64) -> Result<Option<LocalBinanceOptionInstrumentPo>, YuError>;

    async fn create_instrument(&self, instrument: LocalBinanceOptionInstrumentPo) -> Result<u64, YuError>;

    async fn update_instrument(&self, instrument: LocalBinanceOptionInstrumentPo) -> Result<(), YuError>;
}

pub type ClientBinanceOptionInstrumentRepository = Arc<dyn ClientBinanceOptionInstrumentRepositoryTrait>;

pub struct ClientBinanceOptionInstrumentRepositoryImpl {
    pg_pool: PgPool,
}

impl ClientBinanceOptionInstrumentRepositoryImpl {
    pub fn from_pool(pg_pool: PgPool) -> ClientBinanceOptionInstrumentRepository {
        Arc::new(Self { pg_pool })
    }
}

#[async_trait]
impl ClientBinanceOptionInstrumentRepositoryTrait for ClientBinanceOptionInstrumentRepositoryImpl {
    async fn list_all_instruments(&self) -> Result<Vec<LocalBinanceOptionInstrumentPo>, YuError> {
        let rows = sqlx::query(BINANCE_OPTION_INSTRUMENT_COLUMNS).fetch_all(&self.pg_pool).await?;
        rows.into_iter().map(option_instrument_from_row).collect()
    }

    async fn find_instrument_by_server_id(&self, server_id: u64) -> Result<Option<LocalBinanceOptionInstrumentPo>, YuError> {
        let server_id = i64::try_from(server_id).map_err(|_| YuError::new("Binance Option server instrument ID exceeds BIGINT range"))?;
        let row = sqlx::query(
            r#"
            SELECT id, server_id, (EXTRACT(EPOCH FROM expiry_date)::NUMERIC * 1000)::BIGINT AS expiry_date_ms,
                symbol, side, strike_price::TEXT AS strike_price, underlying, unit,
                liquidation_fee_rate::TEXT AS liquidation_fee_rate, min_qty::TEXT AS min_qty, max_qty::TEXT AS max_qty,
                initial_margin::TEXT AS initial_margin, maintenance_margin::TEXT AS maintenance_margin,
                min_initial_margin::TEXT AS min_initial_margin, min_maintenance_margin::TEXT AS min_maintenance_margin,
                price_scale, quantity_scale, quote_asset, contract_type, underlying_type, naked_sell, status, filters
            FROM binance_option_instruments
            WHERE server_id = $1
            "#,
        )
        .bind(server_id)
        .fetch_optional(&self.pg_pool)
        .await?;
        row.map(option_instrument_from_row).transpose()
    }

    async fn create_instrument(&self, instrument: LocalBinanceOptionInstrumentPo) -> Result<u64, YuError> {
        let id = i64::try_from(instrument.id).map_err(|_| YuError::new("local Binance Option instrument ID exceeds BIGINT range"))?;
        let server_id = i64::try_from(instrument.server_id).map_err(|_| YuError::new("Binance Option server instrument ID exceeds BIGINT range"))?;
        let expiry_date = option_expiry_date(instrument.expiry_date)?;
        let inserted_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO binance_option_instruments (
                id, server_id, expiry_date, symbol, side, strike_price, underlying, unit, liquidation_fee_rate,
                min_qty, max_qty, initial_margin, maintenance_margin, min_initial_margin, min_maintenance_margin,
                price_scale, quantity_scale, quote_asset, contract_type, underlying_type, naked_sell, status, filters
            )
            VALUES (
                $1, $2, $3, $4, $5, CAST($6 AS NUMERIC), $7, $8, CAST($9 AS NUMERIC),
                CAST($10 AS NUMERIC), CAST($11 AS NUMERIC), CAST($12 AS NUMERIC), CAST($13 AS NUMERIC),
                CAST($14 AS NUMERIC), CAST($15 AS NUMERIC), $16, $17, $18, $19, $20, $21, $22, $23
            )
            ON CONFLICT (server_id) DO UPDATE SET
                expiry_date = EXCLUDED.expiry_date,
                symbol = EXCLUDED.symbol,
                side = EXCLUDED.side,
                strike_price = EXCLUDED.strike_price,
                underlying = EXCLUDED.underlying,
                unit = EXCLUDED.unit,
                liquidation_fee_rate = EXCLUDED.liquidation_fee_rate,
                min_qty = EXCLUDED.min_qty,
                max_qty = EXCLUDED.max_qty,
                initial_margin = EXCLUDED.initial_margin,
                maintenance_margin = EXCLUDED.maintenance_margin,
                min_initial_margin = EXCLUDED.min_initial_margin,
                min_maintenance_margin = EXCLUDED.min_maintenance_margin,
                price_scale = EXCLUDED.price_scale,
                quantity_scale = EXCLUDED.quantity_scale,
                quote_asset = EXCLUDED.quote_asset,
                contract_type = EXCLUDED.contract_type,
                underlying_type = EXCLUDED.underlying_type,
                naked_sell = EXCLUDED.naked_sell,
                status = EXCLUDED.status,
                filters = EXCLUDED.filters
            RETURNING id
            "#,
        )
        .bind(id)
        .bind(server_id)
        .bind(expiry_date)
        .bind(instrument.symbol)
        .bind(instrument.side)
        .bind(instrument.strike_price)
        .bind(instrument.underlying)
        .bind(instrument.unit)
        .bind(instrument.liquidation_fee_rate)
        .bind(instrument.min_qty)
        .bind(instrument.max_qty)
        .bind(instrument.initial_margin)
        .bind(instrument.maintenance_margin)
        .bind(instrument.min_initial_margin)
        .bind(instrument.min_maintenance_margin)
        .bind(instrument.price_scale)
        .bind(instrument.quantity_scale)
        .bind(instrument.quote_asset)
        .bind(instrument.contract_type)
        .bind(instrument.underlying_type)
        .bind(instrument.naked_sell)
        .bind(instrument.status)
        .bind(sqlx::types::Json(instrument.filters))
        .fetch_one(&self.pg_pool)
        .await?;

        u64::try_from(inserted_id).map_err(|_| YuError::new("negative local Binance Option instrument ID"))
    }

    async fn update_instrument(&self, instrument: LocalBinanceOptionInstrumentPo) -> Result<(), YuError> {
        let id = i64::try_from(instrument.id).map_err(|_| YuError::new("local Binance Option instrument ID exceeds BIGINT range"))?;
        let server_id = i64::try_from(instrument.server_id).map_err(|_| YuError::new("Binance Option server instrument ID exceeds BIGINT range"))?;
        let expiry_date = option_expiry_date(instrument.expiry_date)?;
        let result = sqlx::query(
            r#"
            UPDATE binance_option_instruments
            SET expiry_date = $1, symbol = $2, side = $3, strike_price = CAST($4 AS NUMERIC), underlying = $5, unit = $6,
                liquidation_fee_rate = CAST($7 AS NUMERIC), min_qty = CAST($8 AS NUMERIC), max_qty = CAST($9 AS NUMERIC),
                initial_margin = CAST($10 AS NUMERIC), maintenance_margin = CAST($11 AS NUMERIC),
                min_initial_margin = CAST($12 AS NUMERIC), min_maintenance_margin = CAST($13 AS NUMERIC),
                price_scale = $14, quantity_scale = $15, quote_asset = $16, contract_type = $17, underlying_type = $18,
                naked_sell = $19, status = $20, filters = $21
            WHERE server_id = $22 AND id = $23
            "#,
        )
        .bind(expiry_date)
        .bind(instrument.symbol)
        .bind(instrument.side)
        .bind(instrument.strike_price)
        .bind(instrument.underlying)
        .bind(instrument.unit)
        .bind(instrument.liquidation_fee_rate)
        .bind(instrument.min_qty)
        .bind(instrument.max_qty)
        .bind(instrument.initial_margin)
        .bind(instrument.maintenance_margin)
        .bind(instrument.min_initial_margin)
        .bind(instrument.min_maintenance_margin)
        .bind(instrument.price_scale)
        .bind(instrument.quantity_scale)
        .bind(instrument.quote_asset)
        .bind(instrument.contract_type)
        .bind(instrument.underlying_type)
        .bind(instrument.naked_sell)
        .bind(instrument.status)
        .bind(sqlx::types::Json(instrument.filters))
        .bind(server_id)
        .bind(id)
        .execute(&self.pg_pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(YuError::new("Binance Option instrument disappeared during reconciliation"));
        }
        Ok(())
    }
}

impl ClientBinanceRepositoryImpl {
    pub fn from_pool(pg_pool: PgPool) -> ClientBinanceRepository {
        Arc::new(Self { pg_pool })
    }
}

#[async_trait]
impl ClientBinanceRepositoryTrait for ClientBinanceRepositoryImpl {
    async fn list_all_instruments(&self, instrument_type: BinanceInstrumentType) -> Result<Vec<LocalBinanceInstrumentPo>, YuError> {
        Ok(
            sqlx::query_as::<_, LocalBinanceInstrumentPo>("SELECT * FROM binance_instruments WHERE symbol_type = $1")
                .bind(instrument_type.symbol_type())
                .fetch_all(&self.pg_pool)
                .await?,
        )
    }

    async fn find_instrument_by_server_id(&self, server_id: u64) -> Result<Option<LocalBinanceInstrumentPo>, YuError> {
        let server_id = i64::try_from(server_id).map_err(|_| YuError::new("Binance server instrument ID exceeds BIGINT range"))?;
        Ok(
            sqlx::query_as::<_, LocalBinanceInstrumentPo>("SELECT * FROM binance_instruments WHERE server_id = $1")
                .bind(server_id)
                .fetch_optional(&self.pg_pool)
                .await?,
        )
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

    async fn find_latest_spot_trade_cursor_by_server_id(&self, server_id: u64) -> Result<Option<(u64, i64)>, YuError> {
        let server_id = i64::try_from(server_id).map_err(|_| YuError::new("Binance server instrument ID exceeds BIGINT range"))?;
        let latest: Option<(chrono::DateTime<chrono::Utc>, i64)> = sqlx::query_as(FIND_LATEST_SPOT_TRADE_CURSOR_SQL)
            .bind(server_id)
            .fetch_optional(&self.pg_pool)
            .await?;
        latest
            .map(|(event_time, trade_id)| {
                u64::try_from(event_time.timestamp_millis())
                    .map(|event_time| (event_time, trade_id))
                    .map_err(|_| YuError::new("negative Binance spot trade event time"))
            })
            .transpose()
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
            ON CONFLICT (server_id) DO UPDATE SET
                symbol = EXCLUDED.symbol,
                status = EXCLUDED.status,
                base_asset = EXCLUDED.base_asset,
                quote_asset = EXCLUDED.quote_asset,
                quote_asset_precision = EXCLUDED.quote_asset_precision,
                order_types = EXCLUDED.order_types,
                symbol_type = EXCLUDED.symbol_type,
                on_board_time = EXCLUDED.on_board_time
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

    async fn update_instrument(&self, instrument: LocalBinanceInstrumentPo) -> Result<(), YuError> {
        let id = i64::try_from(instrument.id).map_err(|_| YuError::new("local Binance instrument ID exceeds BIGINT range"))?;
        let server_id = i64::try_from(instrument.server_id).map_err(|_| YuError::new("Binance server instrument ID exceeds BIGINT range"))?;
        let on_board_time = instrument
            .on_board_time
            .map(i64::try_from)
            .transpose()
            .map_err(|_| YuError::new("Binance instrument listing time exceeds BIGINT range"))?;
        let result = sqlx::query(
            r#"
            UPDATE binance_instruments
            SET symbol = $1, status = $2, base_asset = $3, quote_asset = $4, quote_asset_precision = $5,
                order_types = $6, symbol_type = $7, on_board_time = $8
            WHERE server_id = $9 AND id = $10
            "#,
        )
        .bind(instrument.symbol)
        .bind(instrument.status)
        .bind(instrument.base_asset)
        .bind(instrument.quote_asset)
        .bind(instrument.quote_asset_precision)
        .bind(instrument.order_types)
        .bind(instrument.symbol_type)
        .bind(on_board_time)
        .bind(server_id)
        .bind(id)
        .execute(&self.pg_pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(YuError::new("Binance instrument disappeared during reconciliation"));
        }
        Ok(())
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

fn option_expiry_date(expiry_date_ms: u64) -> Result<DateTime<Utc>, YuError> {
    let expiry_date_ms = i64::try_from(expiry_date_ms).map_err(|_| YuError::new("Binance Option expiry timestamp exceeds BIGINT range"))?;
    DateTime::from_timestamp_millis(expiry_date_ms).ok_or_else(|| YuError::new("invalid Binance Option expiry timestamp"))
}

fn option_instrument_from_row(row: sqlx::postgres::PgRow) -> Result<LocalBinanceOptionInstrumentPo, YuError> {
    let id = u64::try_from(row.try_get::<i64, _>("id")?).map_err(|_| YuError::new("negative local Binance Option instrument ID"))?;
    let server_id = u64::try_from(row.try_get::<i64, _>("server_id")?).map_err(|_| YuError::new("negative Binance Option server instrument ID"))?;
    let expiry_date =
        u64::try_from(row.try_get::<i64, _>("expiry_date_ms")?).map_err(|_| YuError::new("negative Binance Option expiry timestamp"))?;
    let filters: sqlx::types::Json<Vec<LocalBinanceOptionSymbolFilterPo>> = row.try_get("filters")?;
    Ok(LocalBinanceOptionInstrumentPo {
        id,
        server_id,
        expiry_date,
        symbol: row.try_get("symbol")?,
        side: row.try_get("side")?,
        strike_price: row.try_get("strike_price")?,
        underlying: row.try_get("underlying")?,
        unit: row.try_get("unit")?,
        liquidation_fee_rate: row.try_get("liquidation_fee_rate")?,
        min_qty: row.try_get("min_qty")?,
        max_qty: row.try_get("max_qty")?,
        initial_margin: row.try_get("initial_margin")?,
        maintenance_margin: row.try_get("maintenance_margin")?,
        min_initial_margin: row.try_get("min_initial_margin")?,
        min_maintenance_margin: row.try_get("min_maintenance_margin")?,
        price_scale: row.try_get("price_scale")?,
        quantity_scale: row.try_get("quantity_scale")?,
        quote_asset: row.try_get("quote_asset")?,
        contract_type: row.try_get("contract_type")?,
        underlying_type: row.try_get("underlying_type")?,
        naked_sell: row.try_get("naked_sell")?,
        status: row.try_get("status")?,
        filters: filters.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // 测试目的：验证 Option expiry 的毫秒时间戳写入 TIMESTAMPTZ 时保持 UTC 毫秒精度。
    // 准备数据/过程：将已知的 Unix 毫秒时间戳转换为 chrono DateTime，再读取回毫秒值。
    // 预期结果：时间戳完全一致，且转换结果时区为 UTC。
    #[test]
    fn option_expiry_date_preserves_milliseconds() {
        let expiry_date = option_expiry_date(1_700_000_000_123).unwrap();

        assert_eq!(expiry_date.timestamp_millis(), 1_700_000_000_123);
        assert_eq!(expiry_date.timezone(), Utc);
    }

    // 测试目的：验证超出 BIGINT 范围的 Option expiry 时间戳会显式报错。
    // 准备数据/过程：向毫秒转 TIMESTAMPTZ helper 传入 u64::MAX。
    // 预期结果：转换返回错误，不截断或回绕该时间戳。
    #[test]
    fn option_expiry_date_rejects_out_of_range_timestamp() {
        let error = option_expiry_date(u64::MAX).unwrap_err();

        assert!(error.to_string().contains("exceeds BIGINT range"));
    }

    // 测试目的：验证 Option filters 能以 JSON 格式无损序列化和反序列化。
    // 准备数据/过程：构造包含可选精度字段的 filter 列表，往返转换为 JSON 字符串。
    // 预期结果：反序列化结果与原 filter 完全相同，满足 JSONB repository 映射。
    #[test]
    fn option_filters_round_trip_through_json() {
        let filters = vec![LocalBinanceOptionSymbolFilterPo {
            filter_type: "PRICE_FILTER".to_string(),
            min_price: Some("0.01".to_string()),
            max_price: None,
            tick_size: Some("0.01".to_string()),
            min_qty: None,
            max_qty: Some("100".to_string()),
            step_size: None,
        }];
        let json = serde_json::to_string(&filters).unwrap();
        let restored: Vec<LocalBinanceOptionSymbolFilterPo> = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, filters);
    }
}
