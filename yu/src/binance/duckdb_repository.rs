use crate::binance::bn_consts::BN_SYMBOL_STATUS_NOT_TRADING;
use crate::binance::models::po::BinanceInstrument;
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
    async fn get_instrument_by_type(&self, inst_type: InstrumentType) -> Result<Vec<BinanceInstrument>, YuError>;

    //
    // a map key is symbol，value is id
    async fn get_map_of_symbol_id(&self, inst_type: InstrumentType) -> Result<HashMap<String, u64>, YuError>;
    async fn insert_instrument(&self, instrument: BinanceInstrument) -> Result<(), YuError>;

    ///
    /// 根据instrument中的symbol进行更新
    ///
    async fn update_instrument(&self, instrument: BinanceInstrument) -> Result<(), YuError>;
    async fn mark_instruments_not_trading(&self, instrument_ids: Vec<u64>) -> Result<(), YuError>;
}

pub type BNInstrumentRepository = Arc<dyn BNInstrumentRepositoryTrait + Send + Sync>;

pub fn get_instrument_repo(provider: Option<DuckDBDSProvider>) -> BNInstrumentRepository {
    Arc::new(BNInstrumentRepositoryImpl {
        provider: provider.unwrap_or_default(),
    })
}

struct BNInstrumentRepositoryImpl {
    provider: DuckDBDSProvider,
}

#[async_trait]
impl BNInstrumentRepositoryTrait for BNInstrumentRepositoryImpl {
    async fn get_instrument_by_type(&self, inst_type: InstrumentType) -> Result<Vec<BinanceInstrument>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare(
            "SELECT id, symbol, status, base_asset, quote_asset, quote_asset_precision, order_types, symbol_type, on_board_time \
             FROM bn_instruments \
             WHERE UPPER(symbol_type) = ? OR (? = 'SWAP' AND UPPER(symbol_type) = 'PERPETUAL');",
        )?;
        let inst_type = inst_type.as_str();
        let mut rows = stmt.query([inst_type, inst_type])?;
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

    async fn get_map_of_symbol_id(&self, inst_type: InstrumentType) -> Result<HashMap<String, u64>, YuError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare(
            "SELECT symbol, id FROM bn_instruments \
             WHERE UPPER(symbol_type) = ? OR (? = 'SWAP' AND UPPER(symbol_type) = 'PERPETUAL');",
        )?;
        let inst_type = inst_type.as_str();
        let mut rows = stmt.query([inst_type, inst_type])?;
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
             WHERE id IN ({placeholders}) AND UPPER(symbol_type) = 'SPOT';"
        );
        let mut params = Vec::with_capacity(instrument_ids.len() + 1);
        params.push(duckdb::types::Value::Text(BN_SYMBOL_STATUS_NOT_TRADING.to_string()));
        params.extend(instrument_ids.into_iter().map(|id| duckdb::types::Value::BigInt(id as i64)));
        conn.execute(&sql, duckdb::params_from_iter(params.iter()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binance::bn_consts::{BN_SYMBOL_STATUS_BREAK, BN_SYMBOL_STATUS_TRADING};
    use crate::binance::db_consts::CREATE_BINANCE_INSTRUMENTS_TABLE;
    use crate::test_utils::create_memory_db_provider;

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

        let repo = get_instrument_repo(Some(provider));
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

        let fetched = repo.get_instrument_by_type(InstrumentType::Spot).await.unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].id, instrument.id);
        assert_eq!(fetched[0].order_types, instrument.order_types);
        assert_eq!(fetched[0].on_board_time, instrument.on_board_time);
        assert_eq!(repo.get_map_of_symbol_id(InstrumentType::Spot).await.unwrap().get("BTCUSDT"), Some(&42));

        let updated = BinanceInstrument {
            status: BN_SYMBOL_STATUS_BREAK.to_string(),
            ..instrument.clone()
        };
        repo.update_instrument(updated.clone()).await.unwrap();

        let fetched = repo.get_instrument_by_type(InstrumentType::Spot).await.unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].status, updated.status);
    }

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

        let repo = get_instrument_repo(Some(provider));
        for (id, symbol, symbol_type) in [
            (1, "BTCUSDT", InstrumentType::Spot),
            (2, "ETHUSDT", InstrumentType::Spot),
            (3, "BTCUSDT", InstrumentType::Swap),
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

        repo.mark_instruments_not_trading(vec![1, 2]).await.unwrap();

        let spot_instruments = repo.get_instrument_by_type(InstrumentType::Spot).await.unwrap();
        assert!(
            spot_instruments
                .iter()
                .all(|instrument| instrument.status == BN_SYMBOL_STATUS_NOT_TRADING)
        );
        let swap_instruments = repo.get_instrument_by_type(InstrumentType::Swap).await.unwrap();
        assert_eq!(swap_instruments[0].status, BN_SYMBOL_STATUS_TRADING);
    }
}
