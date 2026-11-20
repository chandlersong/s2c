use crate::backtest::binance::po::BacktestBinanceInstrumentPo;
use crate::binance::models::local_po::BinanceInstrumentType;
use crate::errors::YuError;
use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;

const LIST_ALL_INSTRUMENTS_SQL: &str = r#"
    SELECT
        inst_id,
        symbol,
        status,
        base_asset,
        quote_asset,
        instrument_type,
        min_trade_size,
        min_trade_quantity,
        tick_size
    FROM backtest_binance_instruments
    WHERE instrument_type = $1
    ORDER BY inst_id
"#;

fn view_instrument_type(instrument_type: BinanceInstrumentType) -> String {
    format!("binance_{}", instrument_type.symbol_type())
}

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait BacktestBinanceInstrumentRepositoryTrait {
    async fn list_all_instruments(&self, instrument_type: BinanceInstrumentType) -> Result<Vec<BacktestBinanceInstrumentPo>, YuError>;
}

pub type BacktestBinanceInstrumentRepository = Arc<dyn BacktestBinanceInstrumentRepositoryTrait + Send + Sync>;

pub struct BacktestBinanceInstrumentRepositoryImpl {
    pg_pool: PgPool,
}

#[async_trait]
impl BacktestBinanceInstrumentRepositoryTrait for BacktestBinanceInstrumentRepositoryImpl {
    async fn list_all_instruments(&self, instrument_type: BinanceInstrumentType) -> Result<Vec<BacktestBinanceInstrumentPo>, YuError> {
        Ok(sqlx::query_as::<_, BacktestBinanceInstrumentPo>(LIST_ALL_INSTRUMENTS_SQL)
            .bind(view_instrument_type(instrument_type))
            .fetch_all(&self.pg_pool)
            .await?)
    }
}

impl BacktestBinanceInstrumentRepositoryImpl {
    pub fn new(pg_pool: PgPool) -> Self {
        Self { pg_pool }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    // 测试目的：验证 Spot 和 Swap 类型能映射到回测视图中的 Binance 类型值。
    // 准备数据/过程：分别转换 BinanceInstrumentType::Spot 和 BinanceInstrumentType::Swap。
    // 预期结果：生成的筛选值分别为 binance_SPOT 和 binance_SWAP。
    #[test]
    fn view_instrument_type_adds_binance_prefix() {
        assert_eq!(view_instrument_type(BinanceInstrumentType::Spot), "binance_SPOT");
        assert_eq!(view_instrument_type(BinanceInstrumentType::Swap), "binance_SWAP");
    }

    // 测试目的：验证查询失败时 repository 会把数据库错误返回给调用方。
    // 准备数据/过程：创建并关闭一个惰性 PostgreSQL 连接池，再调用查询全部交易对的方法。
    // 预期结果：方法返回错误，不会把数据库查询失败伪装成空列表。
    #[tokio::test]
    async fn list_all_instruments_returns_database_error() {
        let pg_pool = PgPoolOptions::new()
            .connect_lazy("postgres://localhost/database")
            .expect("lazy PostgreSQL pool should be created");
        pg_pool.close().await;
        let repository = BacktestBinanceInstrumentRepositoryImpl::new(pg_pool);

        assert!(repository.list_all_instruments(BinanceInstrumentType::Spot).await.is_err());
    }
}
