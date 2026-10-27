use crate::binance::bn_consts::{BN_SYMBOL_STATUS_NOT_TRADING, BN_SYMBOL_STATUS_TRADING};
use crate::binance::models::po::{BinanceInstrument, KlinePo};
use crate::duck_db::DuckDBDSProvider;
use crate::errors::YuError;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use yue::models::InstrumentType;
use yue::query_message::DataSourceProviderTrait;

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
}

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait BinanceKlineRepositoryTrait {
    async fn find_by_inst_id_between(&self, inst_id: u64, start_ms: u64, end_ms: u64) -> Result<Vec<KlinePo>, YuError>;
}

pub type BNInstrumentRepository = Arc<dyn BNInstrumentRepositoryTrait + Send + Sync>;
pub type BinanceKlineRepository = Arc<dyn BinanceKlineRepositoryTrait + Send + Sync>;

pub fn get_instrument_repo_for_production(provider: Option<DuckDBDSProvider>) -> BNInstrumentRepository {
    Arc::new(BNInstrumentRepositoryImpl {
        provider: provider.unwrap_or_default(),
    })
}

pub fn get_default_spot_kline_repo(provider: Option<DuckDBDSProvider>) -> BinanceKlineRepository {
    Arc::new(BinanceKlineRepositoryImpl {
        provider: provider.unwrap_or_default(),
    })
}

struct BinanceKlineRepositoryImpl {
    provider: DuckDBDSProvider,
}

#[async_trait]
impl BinanceKlineRepositoryTrait for BinanceKlineRepositoryImpl {
    async fn find_by_inst_id_between(&self, inst_id: u64, start_ms: u64, end_ms: u64) -> Result<Vec<KlinePo>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare(
            "SELECT id, inst_id, candle_begin_time, open, high, low, close, volume, quote_volume, number_of_trades, \
             taker_buy_base_asset_volume, taker_buy_quote_asset_volume, close_time, interval, first_trade_id, last_trade_id \
             FROM bn_spot_kline WHERE inst_id = ? AND candle_begin_time >= ? AND candle_begin_time <= ? \
             ORDER BY candle_begin_time",
        )?;
        let mut rows = stmt.query(duckdb::params![inst_id as i64, start_ms as i64, end_ms as i64])?;
        let mut klines = Vec::new();
        while let Some(row) = rows.next()? {
            klines.push(KlinePo::from(row));
        }
        Ok(klines)
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
    use crate::binance::db_consts::CREATE_BINANCE_INSTRUMENTS_TABLE;
    use crate::test_utils::create_memory_db_provider;

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
}
