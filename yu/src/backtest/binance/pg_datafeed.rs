use crate::backtest::core::VectorDataFeedTrait;
use crate::errors::YuError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use li::tools::time::unix_time_now_u64_utc;
use polars::prelude::{Column, DataFrame, NamedFrom, Series};
use sqlx::Row;
use sqlx_postgres::PgPool;

///
/// 数数据库中，获取Spot的小时数据。
///
///
///
pub struct SpotHourlyDataFeed {
    minute: usize, // 取第几分钟的数据
    pg_pool: PgPool,
    bt_retention_ms: u64, // 当前小时前的数据
}

impl SpotHourlyDataFeed {
    pub fn new(minute: usize, pg_pool: PgPool, hours: u64) -> Self {
        let bt_retention_ms = hours.saturating_mul(3_600_000);
        SpotHourlyDataFeed {
            minute,
            pg_pool,
            bt_retention_ms,
        }
    }
}

#[async_trait]
impl VectorDataFeedTrait for SpotHourlyDataFeed {
    ///
    /// 从binance_spot_kline_rolling_1h这个view当中获取数据，满足以下条件
    /// 1. 根据bt_retention_ms，计算earliest_ms = now - self.bt_retention_ms，然后取earliest_ms之后的数据
    /// 2. 取minute_index为minute的数据
    ///
    async fn get_all_data(&self) -> Result<DataFrame, YuError> {
        if self.minute > 59 {
            return Err(YuError::new("minute index must be between 0 and 59"));
        }

        let now = unix_time_now_u64_utc();
        let earliest_ms = now.saturating_sub(self.bt_retention_ms);
        let earliest_ms = i64::try_from(earliest_ms).map_err(|error| YuError::new(&error.to_string()))?;
        let earliest_time = DateTime::<Utc>::from_timestamp_millis(earliest_ms).ok_or_else(|| YuError::new("earliest timestamp is out of range"))?;

        let rows = sqlx::query(
            r#"
            SELECT
                id,
                inst_id,
                candle_begin_time,
                window_end,
                open,
                high,
                low,
                close,
                volume,
                quote_volume,
                number_of_trades,
                taker_buy_base_asset_volume,
                taker_buy_quote_asset_volume,
                close_time,
                "interval",
                first_trade_id,
                last_trade_id,
                batch_timestamp,
                minute_count,
                minute_index
            FROM binance_spot_kline_rolling_1h
            WHERE candle_begin_time > $1
              AND minute_index = $2
            ORDER BY candle_begin_time, inst_id
            "#,
        )
        .bind(earliest_time)
        .bind(i32::try_from(self.minute).map_err(|error| YuError::new(&error.to_string()))?)
        .fetch_all(&self.pg_pool)
        .await?;

        let mut id = Vec::with_capacity(rows.len());
        let mut inst_id = Vec::with_capacity(rows.len());
        let mut candle_begin_time = Vec::with_capacity(rows.len());
        let mut window_end = Vec::with_capacity(rows.len());
        let mut open = Vec::with_capacity(rows.len());
        let mut high = Vec::with_capacity(rows.len());
        let mut low = Vec::with_capacity(rows.len());
        let mut close = Vec::with_capacity(rows.len());
        let mut volume = Vec::with_capacity(rows.len());
        let mut quote_volume = Vec::with_capacity(rows.len());
        let mut number_of_trades = Vec::with_capacity(rows.len());
        let mut taker_buy_base_asset_volume = Vec::with_capacity(rows.len());
        let mut taker_buy_quote_asset_volume = Vec::with_capacity(rows.len());
        let mut close_time = Vec::with_capacity(rows.len());
        let mut interval = Vec::with_capacity(rows.len());
        let mut first_trade_id = Vec::with_capacity(rows.len());
        let mut last_trade_id = Vec::with_capacity(rows.len());
        let mut batch_timestamp = Vec::with_capacity(rows.len());
        let mut minute_count = Vec::with_capacity(rows.len());
        let mut minute_index = Vec::with_capacity(rows.len());

        for row in rows {
            id.push(row.try_get::<i64, _>("id")?);
            inst_id.push(row.try_get::<i64, _>("inst_id")?);
            candle_begin_time.push(row.try_get::<DateTime<Utc>, _>("candle_begin_time")?.timestamp_millis());
            window_end.push(row.try_get::<DateTime<Utc>, _>("window_end")?.timestamp_millis());
            open.push(row.try_get::<f64, _>("open")?);
            high.push(row.try_get::<f64, _>("high")?);
            low.push(row.try_get::<f64, _>("low")?);
            close.push(row.try_get::<f64, _>("close")?);
            volume.push(row.try_get::<f64, _>("volume")?);
            quote_volume.push(row.try_get::<f64, _>("quote_volume")?);
            number_of_trades.push(row.try_get::<i64, _>("number_of_trades")?);
            taker_buy_base_asset_volume.push(row.try_get::<f64, _>("taker_buy_base_asset_volume")?);
            taker_buy_quote_asset_volume.push(row.try_get::<f64, _>("taker_buy_quote_asset_volume")?);
            close_time.push(row.try_get::<i64, _>("close_time")?);
            interval.push(row.try_get::<i32, _>("interval")?);
            first_trade_id.push(row.try_get::<Option<i64>, _>("first_trade_id")?);
            last_trade_id.push(row.try_get::<Option<i64>, _>("last_trade_id")?);
            batch_timestamp.push(row.try_get::<i64, _>("batch_timestamp")?);
            minute_count.push(row.try_get::<i64, _>("minute_count")?);
            minute_index.push(row.try_get::<i32, _>("minute_index")?);
        }

        let columns: Vec<Column> = vec![
            Series::new("id".into(), id).into(),
            Series::new("inst_id".into(), inst_id).into(),
            Series::new("candle_begin_time".into(), candle_begin_time).into(),
            Series::new("window_end".into(), window_end).into(),
            Series::new("open".into(), open).into(),
            Series::new("high".into(), high).into(),
            Series::new("low".into(), low).into(),
            Series::new("close".into(), close).into(),
            Series::new("volume".into(), volume).into(),
            Series::new("quote_volume".into(), quote_volume).into(),
            Series::new("number_of_trades".into(), number_of_trades).into(),
            Series::new("taker_buy_base_asset_volume".into(), taker_buy_base_asset_volume).into(),
            Series::new("taker_buy_quote_asset_volume".into(), taker_buy_quote_asset_volume).into(),
            Series::new("close_time".into(), close_time).into(),
            Series::new("interval".into(), interval).into(),
            Series::new("first_trade_id".into(), first_trade_id).into(),
            Series::new("last_trade_id".into(), last_trade_id).into(),
            Series::new("batch_timestamp".into(), batch_timestamp).into(),
            Series::new("minute_count".into(), minute_count).into(),
            Series::new("minute_index".into(), minute_index).into(),
        ];

        DataFrame::new(columns).map_err(|error| YuError::new(&error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 测试目的：验证分钟索引超出小时范围时会立即返回错误。
    // 准备数据/过程：使用不建立真实连接的 PostgreSQL 连接池，并设置 minute 为 60 调用数据读取方法。
    // 预期结果：返回包含分钟索引范围信息的 YuError，且无需访问数据库。
    #[tokio::test]
    async fn get_all_data_rejects_minute_index_out_of_range() {
        let pg_pool = sqlx_postgres::PgPoolOptions::new()
            .connect_lazy("postgres://user:password@localhost/database")
            .expect("lazy PostgreSQL pool should be created");
        let data_feed = SpotHourlyDataFeed {
            minute: 60,
            pg_pool,
            bt_retention_ms: 3_600_000, // 1 hour in milliseconds
        };

        let error = data_feed.get_all_data().await.unwrap_err();

        assert!(matches!(error, YuError::CustomError(message) if message == "minute index must be between 0 and 59"));
    }
}
