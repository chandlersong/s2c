use crate::errors::YuError;
use crate::sync::client::po::binance::{LocalBinanceInstrumentPo, LocalBinanceKlinePo, LocalBinanceTradePo};
use async_trait::async_trait;
use sqlx::{PgPool, Postgres, QueryBuilder};
use std::collections::HashMap;
use std::sync::Arc;

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait ClientBinanceRepositoryTrait: Send + Sync {
    async fn list_all_instruments(&self) -> Result<Vec<LocalBinanceInstrumentPo>, YuError>;
    async fn create_instrument(&self, instrument: LocalBinanceInstrumentPo) -> Result<(), YuError>;
    async fn list_kline_timestamps(&self) -> Result<HashMap<String, u64>, YuError>;
    async fn list_trade_timestamps(&self) -> Result<HashMap<String, u64>, YuError>;
    async fn list_trade_ids(&self) -> Result<HashMap<String, u64>, YuError>;
    async fn insert_klines(&self, rows: Vec<LocalBinanceKlinePo>) -> Result<(), YuError>;
    async fn insert_trades(&self, rows: Vec<LocalBinanceTradePo>) -> Result<(), YuError>;
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
        Ok(sqlx::query_as(
            "SELECT id, server_id, symbol, status, base_asset, quote_asset, quote_asset_precision, order_types, symbol_type, on_board_time \
             FROM binance_instruments ORDER BY symbol, symbol_type",
        )
        .fetch_all(&self.pg_pool)
        .await?)
    }

    async fn create_instrument(&self, instrument: LocalBinanceInstrumentPo) -> Result<(), YuError> {
        sqlx::query(
            "INSERT INTO binance_instruments \
             (id, server_id, symbol, status, base_asset, quote_asset, quote_asset_precision, order_types, symbol_type, on_board_time) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             ON CONFLICT (symbol, symbol_type) DO NOTHING",
        )
        .bind(instrument.id as i64)
        .bind(instrument.server_id as i64)
        .bind(instrument.symbol)
        .bind(instrument.status)
        .bind(instrument.base_asset)
        .bind(instrument.quote_asset)
        .bind(instrument.quote_asset_precision)
        .bind(instrument.order_types)
        .bind(instrument.symbol_type)
        .bind(instrument.on_board_time.map(|value| value as i64))
        .execute(&self.pg_pool)
        .await?;
        Ok(())
    }

    async fn list_kline_timestamps(&self) -> Result<HashMap<String, u64>, YuError> {
        let rows: Vec<(String, i64)> = sqlx::query_as("SELECT symbol, MAX(candle_begin_time) FROM binance_spot_kline_history GROUP BY symbol")
            .fetch_all(&self.pg_pool)
            .await?;
        Ok(rows.into_iter().map(|(symbol, ts)| (symbol, ts as u64)).collect())
    }

    async fn list_trade_timestamps(&self) -> Result<HashMap<String, u64>, YuError> {
        let rows: Vec<(String, i64)> =
            sqlx::query_as("SELECT symbol, MAX(COALESCE(trade_time, event_time)) FROM binance_spot_trade_history GROUP BY symbol")
                .fetch_all(&self.pg_pool)
                .await?;
        Ok(rows.into_iter().map(|(symbol, ts)| (symbol, ts as u64)).collect())
    }

    async fn list_trade_ids(&self) -> Result<HashMap<String, u64>, YuError> {
        let rows: Vec<(String, i64)> = sqlx::query_as("SELECT symbol, MAX(trade_id) FROM binance_spot_trade_history GROUP BY symbol")
            .fetch_all(&self.pg_pool)
            .await?;
        Ok(rows.into_iter().map(|(symbol, id)| (symbol, id as u64)).collect())
    }

    async fn insert_klines(&self, rows: Vec<LocalBinanceKlinePo>) -> Result<(), YuError> {
        if rows.is_empty() {
            return Ok(());
        }

        let mut query = QueryBuilder::<Postgres>::new(
            "INSERT INTO binance_spot_kline_history (id, symbol, candle_begin_time, open, high, low, close, volume, quote_volume, number_of_trades, taker_buy_base_asset_volume, taker_buy_quote_asset_volume, close_time, interval, first_trade_id, last_trade_id, batch_timestamp) ",
        );
        query.push_values(rows, |mut builder, row| {
            builder
                .push_bind(row.id as i64)
                .push_bind(row.symbol)
                .push_bind(row.candle_begin_time as i64)
                .push_bind(row.open)
                .push_bind(row.high)
                .push_bind(row.low)
                .push_bind(row.close)
                .push_bind(row.volume)
                .push_bind(row.quote_volume)
                .push_bind(row.number_of_trades as i64)
                .push_bind(row.taker_buy_base_asset_volume)
                .push_bind(row.taker_buy_quote_asset_volume)
                .push_bind(row.close_time as i64)
                .push_bind(row.interval as i32)
                .push_bind(row.first_trade_id)
                .push_bind(row.last_trade_id)
                .push_bind(row.batch_timestamp as i64);
        });
        query.push(
            " ON CONFLICT (symbol, candle_begin_time) DO UPDATE SET open = EXCLUDED.open, high = EXCLUDED.high, low = EXCLUDED.low, close = EXCLUDED.close, volume = EXCLUDED.volume, quote_volume = EXCLUDED.quote_volume, number_of_trades = EXCLUDED.number_of_trades, taker_buy_base_asset_volume = EXCLUDED.taker_buy_base_asset_volume, taker_buy_quote_asset_volume = EXCLUDED.taker_buy_quote_asset_volume, close_time = EXCLUDED.close_time, interval = EXCLUDED.interval, first_trade_id = EXCLUDED.first_trade_id, last_trade_id = EXCLUDED.last_trade_id, event = COALESCE(EXCLUDED.event, binance_spot_kline_history.event), event_time = COALESCE(EXCLUDED.event_time, binance_spot_kline_history.event_time), batch_timestamp = EXCLUDED.batch_timestamp",
        );
        query.build().execute(&self.pg_pool).await?;
        Ok(())
    }

    async fn insert_trades(&self, rows: Vec<LocalBinanceTradePo>) -> Result<(), YuError> {
        if rows.is_empty() {
            return Ok(());
        }

        let mut query = QueryBuilder::<Postgres>::new(
            "INSERT INTO binance_spot_trade_history (id, event_time, symbol, trade_id, price, qty, trade_time, is_buyer_maker, created_at,batch_timestamp) ",
        );
        query.push_values(rows, |mut builder, row| {
            builder
                .push_bind(row.id as i64)
                .push_bind(row.event_time)
                .push_bind(row.symbol)
                .push_bind(row.trade_id)
                .push_bind(row.price)
                .push_bind(row.qty)
                .push_bind(row.trade_time)
                .push_bind(row.is_buyer_maker)
                .push_bind(row.created_at)
                .push_bind(row.batch_timestamp as i64);
        });
        query.push(
            " ON CONFLICT (symbol, trade_id) DO UPDATE SET event_time = EXCLUDED.event_time, price = EXCLUDED.price, qty = EXCLUDED.qty, trade_time = COALESCE(EXCLUDED.trade_time, binance_spot_trade_history.trade_time), is_buyer_maker = COALESCE(EXCLUDED.is_buyer_maker, binance_spot_trade_history.is_buyer_maker), created_at = EXCLUDED.created_at, event = COALESCE(EXCLUDED.event, binance_spot_trade_history.event), ignore = COALESCE(EXCLUDED.ignore, binance_spot_trade_history.ignore), batch_timestamp = EXCLUDED.batch_timestamp",
        );
        query.build().execute(&self.pg_pool).await?;
        Ok(())
    }
}
