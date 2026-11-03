use crate::binance::bn_backend_service::{get_spot_kline_table, get_swap_funding_rate_table, get_swap_kline_table};
use crate::binance::bn_consts::{BN_SYMBOL_STATUS_NOT_TRADING, BN_SYMBOL_STATUS_TRADING};
use crate::binance::models::po::{BinanceInstrument, FundingRatePo, KlinePo, SpotStreamTradeRecordPo};
use crate::duck_db::DuckDBDSProvider;
use crate::duck_db_tables::DuckTableTableChannel;
use crate::errors::YuError;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use yue::models::InstrumentType;
use yue::query_message::{BatchInsertPayload, DataSourceProviderTrait, InsertPayload, QueryCommand};

fn read_order_types(value: duckdb::types::Value) -> Result<Vec<String>, YuError> {
    let values = match value {
        duckdb::types::Value::List(values) | duckdb::types::Value::Array(values) => values,
        _ => return Err(YuError::new("unexpected DuckDB value for bn_instruments.order_types")),
    };

    values
        .into_iter()
        .map(|value| match value {
            duckdb::types::Value::Text(value) | duckdb::types::Value::Enum(value) => Ok(value),
            _ => Err(YuError::new("unexpected item in bn_instruments.order_types")),
        })
        .collect()
}

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait BNInstrumentRepositoryTrait {
    async fn get_trading_instrument_by_type(&self, inst_type: InstrumentType) -> Result<Vec<BinanceInstrument>, YuError>;

    async fn get_all_instruments_by_type(&self, inst_type: InstrumentType) -> Result<Vec<BinanceInstrument>, YuError>;

    //
    // a map key is symbol，value is id
    async fn get_map_of_symbol_id(&self, inst_types: Vec<InstrumentType>) -> Result<HashMap<String, u64>, YuError>;
    async fn insert_instrument(&self, instrument: BinanceInstrument) -> Result<(), YuError>;

    ///
    /// 根据instrument中的symbol进行更新
    ///
    async fn update_instrument(&self, instrument: BinanceInstrument) -> Result<(), YuError>;
    async fn mark_instruments_not_trading(&self, instrument_ids: Vec<u64>) -> Result<(), YuError>;

    async fn get_by_symbol(&self, symbol: &str, inst_type: InstrumentType) -> Result<Option<BinanceInstrument>, YuError>;
}

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait BinanceKlineRepositoryTrait {
    async fn find_by_inst_id_between(&self, inst_id: u64, start_ms: u64, end_ms: u64) -> Result<Vec<KlinePo>, YuError>;

    ///
    /// 小于的每个instrument最新的kline时间戳，返回map，key是inst_id，value是最新的kline时间戳
    /// 1. 如果instrument在before_ms不存在，返回没有这个instrument的记录。
    /// 2. 如果instrument存在，则返回最大的，且小于等于before_ms的kline时间戳。
    ///
    async fn inst_id_with_lastest_timestamp_before(&self, before_ms: u64) -> Result<HashMap<i64, u64>, YuError>;

    async fn insert_kline(&self, kline: KlinePo) -> Result<(), YuError>;

    async fn batch_insert_kline(&self, klines: Vec<KlinePo>) -> Result<(), YuError>;
}

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait BinanceFundingRateRepositoryTrait {
    async fn find_by_inst_id_between(&self, inst_id: u64, start_ms: u64, end_ms: u64) -> Result<Vec<FundingRatePo>, YuError>;

    ///
    /// 获得每个 instrument 在 before_ms 之前最新的资金费率时间戳。
    /// 如果 instrument 在 before_ms 之前没有记录，则不包含在返回 map 中。
    ///
    async fn inst_id_with_lastest_timestamp_before(&self, before_ms: u64) -> Result<HashMap<i64, u64>, YuError>;

    async fn insert_funding_rate(&self, funding_rate: FundingRatePo) -> Result<(), YuError>;

    async fn batch_insert_funding_rate(&self, funding_rates: Vec<FundingRatePo>) -> Result<(), YuError>;
}

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait BinanceTradeRepositoryTrait {
    async fn find_by_inst_id_between(&self, inst_id: u64, start_ms: u64, end_ms: u64) -> Result<Vec<SpotStreamTradeRecordPo>, YuError>;
}

pub type BNInstrumentRepository = Arc<dyn BNInstrumentRepositoryTrait + Send + Sync>;
pub type BinanceKlineRepository = Arc<dyn BinanceKlineRepositoryTrait + Send + Sync>;
pub type BinanceFundingRateRepository = Arc<dyn BinanceFundingRateRepositoryTrait + Send + Sync>;
pub type BinanceTradeRepository = Arc<dyn BinanceTradeRepositoryTrait + Send + Sync>;

pub fn get_instrument_repo_for_production(provider: Option<DuckDBDSProvider>) -> BNInstrumentRepository {
    Arc::new(BNInstrumentRepositoryImpl {
        provider: provider.unwrap_or_default(),
    })
}

pub fn get_default_spot_kline_repo(provider: Option<DuckDBDSProvider>, channel: Option<DuckTableTableChannel<KlinePo>>) -> BinanceKlineRepository {
    Arc::new(BinanceKlineRepositoryImpl {
        provider: provider.unwrap_or_default(),
        table_name: "bn_spot_kline",
        db: channel.unwrap_or_else(get_spot_kline_table),
    })
}

pub fn get_default_swap_kline_repo(provider: Option<DuckDBDSProvider>, channel: Option<DuckTableTableChannel<KlinePo>>) -> BinanceKlineRepository {
    Arc::new(BinanceKlineRepositoryImpl {
        provider: provider.unwrap_or_default(),
        table_name: "bn_swap_kline",
        db: channel.unwrap_or_else(get_swap_kline_table),
    })
}

pub fn get_default_funding_rate_repo(
    provider: Option<DuckDBDSProvider>,
    channel: Option<DuckTableTableChannel<FundingRatePo>>,
) -> BinanceFundingRateRepository {
    Arc::new(BinanceFundingRateRepositoryImpl {
        provider: provider.unwrap_or_default(),
        db: channel.unwrap_or_else(get_swap_funding_rate_table),
    })
}

pub fn get_default_spot_trade_repo(provider: Option<DuckDBDSProvider>) -> BinanceTradeRepository {
    Arc::new(BinanceTradeRepositoryImpl {
        provider: provider.unwrap_or_default(),
    })
}

struct BinanceFundingRateRepositoryImpl {
    provider: DuckDBDSProvider,
    db: DuckTableTableChannel<FundingRatePo>,
}

#[async_trait]
impl BinanceFundingRateRepositoryTrait for BinanceFundingRateRepositoryImpl {
    async fn find_by_inst_id_between(&self, inst_id: u64, start_ms: u64, end_ms: u64) -> Result<Vec<FundingRatePo>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare(
            "SELECT id, inst_id, funding_rate, funding_time, mark_price \
             FROM bn_swap_funding_rate WHERE inst_id = ? AND funding_time >= ? AND funding_time <= ? \
             ORDER BY funding_time",
        )?;
        let mut rows = stmt.query(duckdb::params![inst_id as i64, start_ms as i64, end_ms as i64])?;
        let mut funding_rates = Vec::new();
        while let Some(row) = rows.next()? {
            funding_rates.push(FundingRatePo::from(row));
        }
        Ok(funding_rates)
    }

    async fn inst_id_with_lastest_timestamp_before(&self, before_ms: u64) -> Result<HashMap<i64, u64>, YuError> {
        let conn = self.provider.acquire()?;
        let before_ms = i64::try_from(before_ms).map_err(|_| YuError::new("before_ms exceeds DuckDB BIGINT range"))?;
        let mut stmt = conn.prepare("SELECT inst_id, MAX(funding_time) FROM bn_swap_funding_rate WHERE funding_time <= ? GROUP BY inst_id")?;
        let mut rows = stmt.query(duckdb::params![before_ms])?;
        let mut timestamps = HashMap::new();

        while let Some(row) = rows.next()? {
            let inst_id: i64 = row.get(0)?;
            let timestamp: i64 = row.get(1)?;
            let timestamp = u64::try_from(timestamp).map_err(|_| YuError::new("negative funding_time in funding rate table"))?;
            timestamps.insert(inst_id, timestamp);
        }

        Ok(timestamps)
    }

    async fn insert_funding_rate(&self, funding_rate: FundingRatePo) -> Result<(), YuError> {
        self.db
            .send(QueryCommand::Insert(InsertPayload::new_no_replay(funding_rate)))
            .await
            .map_err(|error| YuError::CustomError(format!("failed insert binance rate: {error}")))?;
        Ok(())
    }

    async fn batch_insert_funding_rate(&self, funding_rates: Vec<FundingRatePo>) -> Result<(), YuError> {
        self.db
            .send(QueryCommand::BatchInsert(BatchInsertPayload::new_no_replay(funding_rates)))
            .await
            .map_err(|error| YuError::CustomError(format!("failed insert binance rate: {error}")))?;
        Ok(())
    }
}

struct BinanceTradeRepositoryImpl {
    provider: DuckDBDSProvider,
}

#[async_trait]
impl BinanceTradeRepositoryTrait for BinanceTradeRepositoryImpl {
    async fn find_by_inst_id_between(&self, inst_id: u64, start_ms: u64, end_ms: u64) -> Result<Vec<SpotStreamTradeRecordPo>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare(
            "SELECT id, event_time, inst_id, trade_id, CAST(price AS DOUBLE), CAST(qty AS DOUBLE), trade_time, is_buyer_maker, created_at \
             FROM bn_spot_trade WHERE inst_id = ? AND event_time >= ? AND event_time <= ? \
             ORDER BY event_time, trade_id",
        )?;
        let mut rows = stmt.query(duckdb::params![inst_id as i64, start_ms as i64, end_ms as i64])?;
        let mut trades = Vec::new();
        while let Some(row) = rows.next()? {
            trades.push(SpotStreamTradeRecordPo {
                id: row.get(0)?,
                event_time: row.get(1)?,
                inst_id: row.get(2)?,
                trade_id: row.get(3)?,
                price: row.get(4)?,
                qty: row.get(5)?,
                trade_time: row.get(6)?,
                is_buyer_maker: row.get(7)?,
                created_at: row.get(8)?,
            });
        }
        Ok(trades)
    }
}

struct BinanceKlineRepositoryImpl {
    provider: DuckDBDSProvider,
    table_name: &'static str,
    db: DuckTableTableChannel<KlinePo>,
}

#[async_trait]
impl BinanceKlineRepositoryTrait for BinanceKlineRepositoryImpl {
    async fn find_by_inst_id_between(&self, inst_id: u64, start_ms: u64, end_ms: u64) -> Result<Vec<KlinePo>, YuError> {
        let conn = self.provider.acquire()?;
        let sql = format!(
            "SELECT id, inst_id, candle_begin_time, open, high, low, close, volume, quote_volume, number_of_trades, \
             taker_buy_base_asset_volume, taker_buy_quote_asset_volume, close_time, interval, first_trade_id, last_trade_id \
             FROM {} WHERE inst_id = ? AND candle_begin_time >= ? AND candle_begin_time <= ? \
             ORDER BY candle_begin_time",
            self.table_name
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(duckdb::params![inst_id as i64, start_ms as i64, end_ms as i64])?;
        let mut klines = Vec::new();
        while let Some(row) = rows.next()? {
            klines.push(KlinePo::from(row));
        }
        Ok(klines)
    }

    async fn inst_id_with_lastest_timestamp_before(&self, before_ms: u64) -> Result<HashMap<i64, u64>, YuError> {
        let conn = self.provider.acquire()?;
        let before_ms = i64::try_from(before_ms).map_err(|_| YuError::new("before_ms exceeds DuckDB BIGINT range"))?;
        let sql = format!(
            "SELECT inst_id, MAX(candle_begin_time) FROM {} WHERE candle_begin_time <= ? GROUP BY inst_id",
            self.table_name
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(duckdb::params![before_ms])?;
        let mut timestamps = HashMap::new();

        while let Some(row) = rows.next()? {
            let inst_id: i64 = row.get(0)?;
            let timestamp: i64 = row.get(1)?;
            let timestamp = u64::try_from(timestamp).map_err(|_| YuError::new("negative close_time in kline table"))?;
            timestamps.insert(inst_id, timestamp);
        }

        Ok(timestamps)
    }

    async fn insert_kline(&self, kline: KlinePo) -> Result<(), YuError> {
        self.db
            .send(QueryCommand::Insert(InsertPayload::new_no_replay(kline)))
            .await
            .map_err(|error| YuError::CustomError(format!("failed insert binance kline: {error}")))?;
        Ok(())
    }

    async fn batch_insert_kline(&self, klines: Vec<KlinePo>) -> Result<(), YuError> {
        self.db
            .send(QueryCommand::BatchInsert(BatchInsertPayload::new_no_replay(klines)))
            .await
            .map_err(|error| YuError::CustomError(format!("failed insert binance kline: {error}")))?;
        Ok(())
    }
}

struct BNInstrumentRepositoryImpl {
    provider: DuckDBDSProvider,
}

#[async_trait]
impl BNInstrumentRepositoryTrait for BNInstrumentRepositoryImpl {
    async fn get_trading_instrument_by_type(&self, inst_type: InstrumentType) -> Result<Vec<BinanceInstrument>, YuError> {
        self.query_instruments_by_type(inst_type, true)
    }

    async fn get_all_instruments_by_type(&self, inst_type: InstrumentType) -> Result<Vec<BinanceInstrument>, YuError> {
        self.query_instruments_by_type(inst_type, false)
    }

    async fn get_map_of_symbol_id(&self, inst_types: Vec<InstrumentType>) -> Result<HashMap<String, u64>, YuError> {
        if inst_types.is_empty() {
            return Ok(HashMap::new());
        }

        let conn = self.provider.acquire()?;
        let placeholders = vec!["?"; inst_types.len()].join(", ");
        let sql = format!(
            "SELECT symbol, id FROM bn_instruments \
             WHERE symbol_type IN ({placeholders});"
        );
        let params = inst_types
            .iter()
            .map(|inst_type| duckdb::types::Value::Text(inst_type.as_str().to_string()))
            .collect::<Vec<_>>();
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(duckdb::params_from_iter(params.iter()))?;
        let mut instruments = HashMap::new();

        while let Some(row) = rows.next()? {
            let symbol: String = row.get(0)?;
            let id: i64 = row.get(1)?;
            instruments.insert(symbol, id as u64);
        }

        Ok(instruments)
    }

    async fn insert_instrument(&self, instrument: BinanceInstrument) -> Result<(), YuError> {
        let conn = self.provider.acquire()?;
        let order_types = format!(
            "CAST([{}] AS VARCHAR[])",
            instrument
                .order_types
                .iter()
                .map(|order_type| format!("'{}'", order_type.replace('\'', "''")))
                .collect::<Vec<_>>()
                .join(",")
        );
        let sql = format!(
            "INSERT INTO bn_instruments \
             (id, symbol, status, base_asset, quote_asset, quote_asset_precision, order_types, symbol_type, on_board_time) \
             VALUES (?, ?, ?, ?, ?, ?, {order_types}, ?, ?);"
        );
        conn.execute(
            &sql,
            duckdb::params![
                instrument.id,
                instrument.symbol,
                instrument.status,
                instrument.base_asset,
                instrument.quote_asset,
                instrument.quote_asset_precision,
                instrument.symbol_type.to_string(),
                instrument.on_board_time.map(|time| time as i64),
            ],
        )?;
        Ok(())
    }

    async fn update_instrument(&self, instrument: BinanceInstrument) -> Result<(), YuError> {
        let conn = self.provider.acquire()?;
        let order_types = format!(
            "CAST([{}] AS VARCHAR[])",
            instrument
                .order_types
                .iter()
                .map(|order_type| format!("'{}'", order_type.replace('\'', "''")))
                .collect::<Vec<_>>()
                .join(",")
        );
        let sql = format!(
            "UPDATE bn_instruments \
             SET status = ?, base_asset = ?, quote_asset = ?, quote_asset_precision = ?, \
                 order_types = {order_types}, on_board_time = ? \
             WHERE symbol = ? AND \
                 (UPPER(symbol_type) = ? OR (? = 'SWAP' AND UPPER(symbol_type) = 'PERPETUAL'));"
        );
        conn.execute(
            &sql,
            duckdb::params![
                instrument.status,
                instrument.base_asset,
                instrument.quote_asset,
                instrument.quote_asset_precision,
                instrument.on_board_time.map(|time| time as i64),
                instrument.symbol,
                instrument.symbol_type.to_string(),
                instrument.symbol_type.to_string(),
            ],
        )?;
        Ok(())
    }

    async fn mark_instruments_not_trading(&self, instrument_ids: Vec<u64>) -> Result<(), YuError> {
        if instrument_ids.is_empty() {
            return Ok(());
        }

        let conn = self.provider.acquire()?;
        let placeholders = vec!["?"; instrument_ids.len()].join(", ");
        let sql = format!(
            "UPDATE bn_instruments SET status = ? \
             WHERE id IN ({placeholders}) ;"
        );
        let mut params = Vec::with_capacity(instrument_ids.len() + 1);
        params.push(duckdb::types::Value::Text(BN_SYMBOL_STATUS_NOT_TRADING.to_string()));
        params.extend(instrument_ids.into_iter().map(|id| duckdb::types::Value::BigInt(id as i64)));
        conn.execute(&sql, duckdb::params_from_iter(params.iter()))?;
        Ok(())
    }

    async fn get_by_symbol(&self, symbol: &str, inst_type: InstrumentType) -> Result<Option<BinanceInstrument>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare(
            "SELECT id, symbol, status, base_asset, quote_asset, quote_asset_precision, order_types, symbol_type, on_board_time \
             FROM bn_instruments \
             WHERE symbol = ? AND (UPPER(symbol_type) = ? OR (? = 'SWAP' AND UPPER(symbol_type) = 'PERPETUAL')) \
             LIMIT 1;",
        )?;
        let inst_type = inst_type.as_str();
        let mut rows = stmt.query(duckdb::params![symbol, inst_type, inst_type])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };

        Ok(Some(BinanceInstrument {
            id: row.get(0)?,
            symbol: row.get(1)?,
            status: row.get(2)?,
            base_asset: row.get(3)?,
            quote_asset: row.get(4)?,
            quote_asset_precision: row.get(5)?,
            order_types: read_order_types(row.get(6)?)?,
            symbol_type: InstrumentType::from_symbol_type(&row.get::<_, String>(7)?)
                .ok_or_else(|| YuError::new("unsupported bn_instruments.symbol_type"))?,
            on_board_time: row.get::<_, Option<i64>>(8)?.map(|time| time as u64),
        }))
    }
}

impl BNInstrumentRepositoryImpl {
    fn query_instruments_by_type(&self, inst_type: InstrumentType, trading_only: bool) -> Result<Vec<BinanceInstrument>, YuError> {
        let conn = self.provider.acquire()?;
        let status_filter = if trading_only { " AND UPPER(status) = ?" } else { "" };
        let sql = format!(
            "SELECT id, symbol, status, base_asset, quote_asset, quote_asset_precision, order_types, symbol_type, on_board_time \
             FROM bn_instruments \
             WHERE (UPPER(symbol_type) = ? OR (? = 'SWAP' AND UPPER(symbol_type) = 'PERPETUAL')){status_filter};"
        );
        let inst_type = inst_type.as_str();
        let mut params = vec![
            duckdb::types::Value::Text(inst_type.to_string()),
            duckdb::types::Value::Text(inst_type.to_string()),
        ];
        if trading_only {
            params.push(duckdb::types::Value::Text(BN_SYMBOL_STATUS_TRADING.to_string()));
        }
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(duckdb::params_from_iter(params.iter()))?;
        let mut instruments = Vec::new();

        while let Some(row) = rows.next()? {
            instruments.push(BinanceInstrument {
                id: row.get(0)?,
                symbol: row.get(1)?,
                status: row.get(2)?,
                base_asset: row.get(3)?,
                quote_asset: row.get(4)?,
                quote_asset_precision: row.get(5)?,
                order_types: read_order_types(row.get(6)?)?,
                symbol_type: InstrumentType::from_symbol_type(&row.get::<_, String>(7)?)
                    .ok_or_else(|| YuError::new("unsupported bn_instruments.symbol_type"))?,
                on_board_time: row.get::<_, Option<i64>>(8)?.map(|time| time as u64),
            });
        }

        Ok(instruments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binance::bn_consts::{BN_SYMBOL_STATUS_BREAK, BN_SYMBOL_STATUS_TRADING};
    use crate::binance::db_consts::{CREATE_BINANCE_INSTRUMENTS_TABLE, CREATE_FUNDING_RATE_TABLE, CREATE_SPOT_KLINE_TABLE, CREATE_SWAP_KLINE_TABLE};
    use crate::test_utils::create_memory_db_provider;
    use tokio::sync::mpsc;

    // 测试目的：验证 instrument 的全量查询和交易中查询会按 status 正确区分。
    // 准备数据/过程：插入 BTCUSDT Spot，先查询交易中列表，再将其更新为 BREAK 后重新查询。
    // 预期结果：全量查询始终返回该 instrument；交易中查询只在 status 为 TRADING 时返回它。
    #[tokio::test]
    async fn instrument_repository_supports_crud_queries() {
        let provider = create_memory_db_provider();
        let conn = provider.acquire().unwrap();
        conn.execute(CREATE_BINANCE_INSTRUMENTS_TABLE.split(';').next().unwrap(), []).unwrap();
        conn.execute(
            "CREATE UNIQUE INDEX idx_bn_instruments_symbol_type ON bn_instruments(symbol, symbol_type);",
            [],
        )
        .unwrap();
        drop(conn);

        let repo = get_instrument_repo_for_production(Some(provider));
        let instrument = BinanceInstrument {
            id: 42,
            symbol: "BTCUSDT".to_string(),
            status: BN_SYMBOL_STATUS_TRADING.to_string(),
            base_asset: "BTC".to_string(),
            quote_asset: "USDT".to_string(),
            quote_asset_precision: 8,
            order_types: vec!["LIMIT".to_string(), "MARKET".to_string()],
            symbol_type: InstrumentType::Spot,
            on_board_time: Some(1_700_000_000_000),
        };

        repo.insert_instrument(instrument.clone()).await.unwrap();

        let fetched = repo.get_all_instruments_by_type(InstrumentType::Spot).await.unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].id, instrument.id);
        assert_eq!(fetched[0].order_types, instrument.order_types);
        assert_eq!(fetched[0].on_board_time, instrument.on_board_time);
        assert_eq!(repo.get_trading_instrument_by_type(InstrumentType::Spot).await.unwrap().len(), 1);
        assert_eq!(
            repo.get_map_of_symbol_id(vec![InstrumentType::Spot]).await.unwrap().get("BTCUSDT"),
            Some(&42)
        );

        let updated = BinanceInstrument {
            status: BN_SYMBOL_STATUS_BREAK.to_string(),
            ..instrument.clone()
        };
        repo.update_instrument(updated.clone()).await.unwrap();

        let fetched = repo.get_all_instruments_by_type(InstrumentType::Spot).await.unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].status, updated.status);
        assert!(repo.get_trading_instrument_by_type(InstrumentType::Spot).await.unwrap().is_empty());
    }

    // 测试目的：验证按 symbol 和类型查询时可区分同名 Spot 与 Swap instrument，并对不存在的记录返回 None。
    // 准备数据/过程：在内存 DuckDB 创建 instrument 表，插入同名的 Spot 和 Swap instrument，再分别按类型及不存在的 symbol 查询。
    // 预期结果：Spot 和 Swap 查询分别返回对应记录；不存在的 symbol 返回 None。
    #[tokio::test]
    async fn instrument_repository_gets_instrument_by_symbol_and_type() {
        let provider = create_memory_db_provider();
        let conn = provider.acquire().unwrap();
        conn.execute(CREATE_BINANCE_INSTRUMENTS_TABLE.split(';').next().unwrap(), []).unwrap();
        conn.execute(
            "CREATE UNIQUE INDEX idx_bn_instruments_symbol_type ON bn_instruments(symbol, symbol_type);",
            [],
        )
        .unwrap();
        drop(conn);

        let repo = get_instrument_repo_for_production(Some(provider));
        for (id, symbol_type) in [(42, InstrumentType::Spot), (43, InstrumentType::Swap)] {
            repo.insert_instrument(BinanceInstrument {
                id,
                symbol: "BTCUSDT".to_string(),
                status: BN_SYMBOL_STATUS_TRADING.to_string(),
                base_asset: "BTC".to_string(),
                quote_asset: "USDT".to_string(),
                quote_asset_precision: 8,
                order_types: vec!["LIMIT".to_string(), "MARKET".to_string()],
                symbol_type,
                on_board_time: Some(1_700_000_000_000),
            })
            .await
            .unwrap();
        }

        let spot = repo.get_by_symbol("BTCUSDT", InstrumentType::Spot).await.unwrap().unwrap();
        assert_eq!(spot.id, 42);
        assert_eq!(spot.symbol_type.to_string(), "SPOT");
        assert_eq!(spot.order_types, vec!["LIMIT".to_string(), "MARKET".to_string()]);

        let swap = repo.get_by_symbol("BTCUSDT", InstrumentType::Swap).await.unwrap().unwrap();
        assert_eq!(swap.id, 43);
        assert_eq!(swap.symbol_type.to_string(), "SWAP");
        assert_eq!(swap.on_board_time, Some(1_700_000_000_000));

        assert!(repo.get_by_symbol("ETHUSDT", InstrumentType::Spot).await.unwrap().is_none());
    }

    // 测试目的：验证按 symbol 查询时数据库错误会显式返回。
    // 准备数据/过程：使用未创建 instrument 表的内存 DuckDB 执行查询。
    // 预期结果：查询返回 Err，而不是伪装成未找到。
    #[tokio::test]
    async fn instrument_repository_propagates_get_by_symbol_query_errors() {
        let repo = get_instrument_repo_for_production(Some(create_memory_db_provider()));

        assert!(repo.get_by_symbol("BTCUSDT", InstrumentType::Spot).await.is_err());
    }

    // 测试目的：验证按类型批量标记非交易和多类型 symbol-id 查询的结果正确。
    // 准备数据/过程：插入两个 Spot 和一个 Swap 交易中 instrument，再将两个 Spot 标记为非交易并查询 Spot、Swap 的 symbol-id。
    // 预期结果：symbol-id 查询同时包含 Spot 和 Swap；全量查询仍返回对应类型的所有 instrument，交易中查询只返回未被标记的 Swap。
    #[tokio::test]
    async fn marks_multiple_spot_instruments_not_trading_in_one_call() {
        let provider = create_memory_db_provider();
        let conn = provider.acquire().unwrap();
        conn.execute(CREATE_BINANCE_INSTRUMENTS_TABLE.split(';').next().unwrap(), []).unwrap();
        conn.execute(
            "CREATE UNIQUE INDEX idx_bn_instruments_symbol_type ON bn_instruments(symbol, symbol_type);",
            [],
        )
        .unwrap();
        drop(conn);

        let repo = get_instrument_repo_for_production(Some(provider));
        for (id, symbol, symbol_type) in [
            (1, "BTCUSDT", InstrumentType::Spot),
            (2, "ETHUSDT", InstrumentType::Spot),
            (3, "BNBUSDT", InstrumentType::Swap),
        ] {
            repo.insert_instrument(BinanceInstrument {
                id,
                symbol: symbol.to_string(),
                status: BN_SYMBOL_STATUS_TRADING.to_string(),
                base_asset: "BTC".to_string(),
                quote_asset: "USDT".to_string(),
                quote_asset_precision: 8,
                order_types: vec!["LIMIT".to_string()],
                symbol_type,
                on_board_time: None,
            })
            .await
            .unwrap();
        }

        let symbol_ids = repo.get_map_of_symbol_id(vec![InstrumentType::Spot, InstrumentType::Swap]).await.unwrap();
        assert_eq!(symbol_ids.get("ETHUSDT"), Some(&2));
        assert_eq!(symbol_ids.get("BNBUSDT"), Some(&3));
        assert!(repo.get_map_of_symbol_id(vec![]).await.unwrap().is_empty());

        repo.mark_instruments_not_trading(vec![1, 2]).await.unwrap();

        let spot_instruments = repo.get_all_instruments_by_type(InstrumentType::Spot).await.unwrap();
        assert!(
            spot_instruments
                .iter()
                .all(|instrument| instrument.status == BN_SYMBOL_STATUS_NOT_TRADING)
        );
        let swap_instruments = repo.get_all_instruments_by_type(InstrumentType::Swap).await.unwrap();
        assert_eq!(swap_instruments[0].status, BN_SYMBOL_STATUS_TRADING);
        assert!(repo.get_trading_instrument_by_type(InstrumentType::Spot).await.unwrap().is_empty());
        assert_eq!(repo.get_trading_instrument_by_type(InstrumentType::Swap).await.unwrap().len(), 1);
    }

    // 测试目的：验证 Swap Kline repository 从 bn_swap_kline 读取指定 instrument 和时间范围的数据。
    // 准备数据/过程：在内存 DuckDB 创建 Swap Kline 表，插入一条符合条件的记录，再通过 Swap repository 查询。
    // 预期结果：查询返回唯一记录，且 instrument ID、毫秒开盘时间和收盘价保持正确。
    #[tokio::test]
    async fn swap_kline_repository_reads_swap_table() {
        let (tx, _) = mpsc::channel(1);
        let provider = create_memory_db_provider();
        let conn = provider.acquire().unwrap();
        for statement in CREATE_SWAP_KLINE_TABLE.split(';').map(str::trim).filter(|sql| !sql.is_empty()) {
            conn.execute(statement, []).unwrap();
        }
        conn.execute(
            "INSERT INTO bn_swap_kline VALUES (3, 42, 1700000000000, 100, 110, 90, 105, 2, 210, 20, 1, 105, 1700000059999, 0, -1, -1)",
            [],
        )
        .unwrap();
        drop(conn);

        let repo = get_default_swap_kline_repo(Some(provider), Some(tx));
        let klines = repo.find_by_inst_id_between(42, 1_700_000_000_000, 1_700_000_000_000).await.unwrap();

        assert_eq!(klines.len(), 1);
        assert_eq!(klines[0].inst_id, 42);
        assert_eq!(klines[0].candle_begin_time, 1_700_000_000_000);
        assert_eq!(klines[0].close, 105.0);
    }

    // 测试目的：验证 Spot 和 Swap repository 返回 before_ms 之前每个 instrument 的最新 close_time。
    // 准备数据/过程：在内存 DuckDB 创建两张 Kline 表，为多 instrument 插入 before_ms 前后的记录，再调用两个 repository。
    // 预期结果：仅返回 close_time 小于等于 before_ms 的 instrument 记录，并选择其中最大的 close_time。
    #[tokio::test]
    async fn kline_repository_returns_latest_close_time_before_timestamp_per_instrument() {
        let (spot_tx, _) = mpsc::channel(1);
        let (swap_tx, _) = mpsc::channel(1);
        let provider = create_memory_db_provider();
        let conn = provider.acquire().unwrap();
        for statement in CREATE_SPOT_KLINE_TABLE
            .split(';')
            .map(str::trim)
            .filter(|sql| !sql.is_empty())
            .chain(CREATE_SWAP_KLINE_TABLE.split(';').map(str::trim).filter(|sql| !sql.is_empty()))
        {
            conn.execute(statement, []).unwrap();
        }
        conn.execute(
            "INSERT INTO bn_spot_kline (id, inst_id, candle_begin_time, close_time) VALUES \
             (1, 42, 1000, 1999), (2, 42, 2000, 2999), (3, 43, 1500, 2499), \
             (7, 43, 2500, 3499), (8, 45, 4000, 4999)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO bn_swap_kline (id, inst_id, candle_begin_time, close_time) VALUES \
             (4, 42, 1000, 1999), (5, 42, 3000, 3999), (6, 44, 2500, 2499), \
             (7, 44, 3500, 4499), (8, 46, 5000, 5999)",
            [],
        )
        .unwrap();
        drop(conn);

        let spot_repo = get_default_spot_kline_repo(Some(provider.clone()), Some(spot_tx));
        let swap_repo = get_default_swap_kline_repo(Some(provider), Some(swap_tx));

        assert_eq!(
            spot_repo.inst_id_with_lastest_timestamp_before(2999).await.unwrap(),
            HashMap::from([(42, 2000), (43, 2500)])
        );
        assert_eq!(
            swap_repo.inst_id_with_lastest_timestamp_before(2999).await.unwrap(),
            HashMap::from([(42, 1000), (44, 2500)])
        );
    }

    // 测试目的：验证已创建但无 Kline 数据的表会得到空结果。
    // 准备数据/过程：创建空的 Spot Kline 表并调用 repository 查询最新时间戳。
    // 预期结果：查询成功并返回空 HashMap。
    #[tokio::test]
    async fn kline_repository_returns_empty_map_for_empty_table() {
        let (tx, _) = mpsc::channel(1);
        let provider = create_memory_db_provider();
        let conn = provider.acquire().unwrap();
        conn.execute(CREATE_SPOT_KLINE_TABLE.split(';').next().unwrap(), []).unwrap();
        drop(conn);

        let repo = get_default_spot_kline_repo(Some(provider), Some(tx));

        assert!(repo.inst_id_with_lastest_timestamp_before(2999).await.unwrap().is_empty());
    }

    // 测试目的：验证查询 Kline 表失败时 repository 会返回错误。
    // 准备数据/过程：不创建 bn_spot_kline 表，直接查询最新时间戳。
    // 预期结果：查询返回 Err，而不是伪装成空结果。
    #[tokio::test]
    async fn kline_repository_propagates_latest_timestamp_query_errors() {
        let (tx, _) = mpsc::channel(1);
        let repo = get_default_spot_kline_repo(Some(create_memory_db_provider()), Some(tx));

        assert!(repo.inst_id_with_lastest_timestamp_before(2999).await.is_err());
    }

    // 测试目的：验证 Spot Kline repository 使用调用方提供的写入 channel。
    // 准备数据/过程：创建内存 channel，将其注入 Spot Kline repository，再插入一条 Kline。
    // 预期结果：插入命令通过注入的 channel 发出，且 batch 中包含该 Kline。
    #[tokio::test]
    async fn spot_kline_repository_uses_injected_channel() {
        let (tx, mut rx) = mpsc::channel(1);
        let repo = get_default_spot_kline_repo(Some(create_memory_db_provider()), Some(tx));
        let kline = KlinePo {
            id: 1,
            inst_id: 42,
            candle_begin_time: 1_700_000_000_000,
            open: 100.0,
            high: 110.0,
            low: 90.0,
            close: 105.0,
            volume: 2.0,
            quote_volume: 210.0,
            number_of_trades: 20,
            taker_buy_base_asset_volume: 1.0,
            taker_buy_quote_asset_volume: 105.0,
            close_time: 1_700_000_059_999,
            interval: 0,
            first_trade_id: None,
            last_trade_id: None,
        };

        repo.insert_kline(kline.clone()).await.unwrap();

        match rx.recv().await.unwrap() {
            QueryCommand::Insert(payload) => {
                assert_eq!(payload.data.id, kline.id);
                assert_eq!(payload.data.inst_id, kline.inst_id);
            }
            _ => panic!("expected insert command"),
        }
    }

    // 测试目的：验证 Funding Rate repository 按 instrument 和含端点的毫秒时间范围读取，并通过注入的 channel 写入记录。
    // 准备数据/过程：在内存 DuckDB 创建资金费率表并插入多条记录，查询指定范围，再将新记录插入 channel 并接收 batch 命令。
    // 预期结果：查询按 funding_time 排序且 nullable mark_price 保持 None；新增记录通过注入 channel 发出。
    #[tokio::test]
    async fn funding_rate_repository_reads_inclusive_time_range_in_order() {
        let provider = create_memory_db_provider();
        let conn = provider.acquire().unwrap();
        for statement in CREATE_FUNDING_RATE_TABLE.split(';').map(str::trim).filter(|sql| !sql.is_empty()) {
            conn.execute(statement, []).unwrap();
        }
        conn.execute(
            "INSERT INTO bn_swap_funding_rate VALUES (1, 42, 0.0001, 1000, NULL), (2, 42, 0.0002, 2000, 100.5), (3, 43, 0.0003, 1500, 100.0)",
            [],
        )
        .unwrap();
        drop(conn);

        let (tx, mut rx) = mpsc::channel(1);
        let repo = get_default_funding_rate_repo(Some(provider), Some(tx));
        let funding_rates = repo.find_by_inst_id_between(42, 1000, 2000).await.unwrap();

        assert_eq!(funding_rates.len(), 2);
        assert_eq!(funding_rates[0].funding_time, 1000);
        assert_eq!(funding_rates[0].mark_price, None);
        assert_eq!(funding_rates[1].funding_time, 2000);
        assert_eq!(funding_rates[1].mark_price, Some(100.5));

        let funding_rate = FundingRatePo {
            id: 4,
            inst_id: 42,
            funding_rate: 0.0004,
            funding_time: 3000,
            mark_price: Some(101.0),
        };
        repo.insert_funding_rate(funding_rate.clone()).await.unwrap();

        match rx.recv().await.unwrap() {
            QueryCommand::Insert(payload) => {
                assert_eq!(payload.data.id, funding_rate.id);
                assert_eq!(payload.data.funding_time, funding_rate.funding_time);
            }
            _ => panic!("expected insert command"),
        }
    }

    // 测试目的：验证 Funding Rate repository 返回 before_ms 之前每个 instrument 的最新 funding_time。
    // 准备数据/过程：在内存 DuckDB 创建资金费率表，为多个 instrument 插入 before_ms 前后的记录，再查询最新时间戳。
    // 预期结果：仅返回 funding_time 小于等于 before_ms 的 instrument 记录，并选择其中最大的 funding_time。
    #[tokio::test]
    async fn funding_rate_repository_returns_latest_timestamp_before_timestamp_per_instrument() {
        let provider = create_memory_db_provider();
        let conn = provider.acquire().unwrap();
        for statement in CREATE_FUNDING_RATE_TABLE.split(';').map(str::trim).filter(|sql| !sql.is_empty()) {
            conn.execute(statement, []).unwrap();
        }
        conn.execute(
            "INSERT INTO bn_swap_funding_rate VALUES \
             (1, 42, 0.0001, 1000, NULL), (2, 42, 0.0002, 3000, 100.5), \
             (3, 43, 0.0003, 2000, 100.0), (4, 44, 0.0004, 5000, 101.0)",
            [],
        )
        .unwrap();
        drop(conn);

        let (tx, _) = mpsc::channel(1);
        let repo = get_default_funding_rate_repo(Some(provider), Some(tx));

        assert_eq!(
            repo.inst_id_with_lastest_timestamp_before(3000).await.unwrap(),
            HashMap::from([(42, 3000), (43, 2000)])
        );
    }

    // 测试目的：验证空资金费率表返回空结果。
    // 准备数据/过程：创建空的资金费率表并调用 repository 查询最新时间戳。
    // 预期结果：查询成功并返回空 HashMap。
    #[tokio::test]
    async fn funding_rate_repository_returns_empty_map_for_empty_table() {
        let provider = create_memory_db_provider();
        let conn = provider.acquire().unwrap();
        conn.execute(CREATE_FUNDING_RATE_TABLE.split(';').next().unwrap(), []).unwrap();
        drop(conn);

        let (tx, _) = mpsc::channel(1);
        let repo = get_default_funding_rate_repo(Some(provider), Some(tx));

        assert!(repo.inst_id_with_lastest_timestamp_before(3000).await.unwrap().is_empty());
    }

    // 测试目的：验证查询资金费率表失败时 repository 会返回错误。
    // 准备数据/过程：不创建 bn_swap_funding_rate 表，直接查询最新时间戳。
    // 预期结果：查询返回 Err，而不是伪装成空结果。
    #[tokio::test]
    async fn funding_rate_repository_propagates_latest_timestamp_query_errors() {
        let (tx, _) = mpsc::channel(1);
        let repo = get_default_funding_rate_repo(Some(create_memory_db_provider()), Some(tx));

        assert!(repo.inst_id_with_lastest_timestamp_before(3000).await.is_err());
    }
}
