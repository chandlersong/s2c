use crate::models::BacktestInstTrait;
use sqlx::{Row, postgres::PgRow};
use yue::models::Decimal;

#[derive(Clone, Debug, PartialEq)]
pub struct BacktestBinanceInstrumentPo {
    pub inst_id: u64,
    pub symbol: String,
    pub status: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub instrument_type: String,
    pub min_trade_size: Decimal,
    pub min_trade_quantity: Decimal,
    pub tick_size: Decimal,
}

impl<'r> sqlx::FromRow<'r, PgRow> for BacktestBinanceInstrumentPo {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let inst_id = row.try_get::<i64, _>("inst_id")?;
        let inst_id = u64::try_from(inst_id).map_err(|error| sqlx::Error::Decode(Box::new(error)))?;

        Ok(Self {
            inst_id,
            symbol: row.try_get("symbol")?,
            status: row.try_get("status")?,
            base_asset: row.try_get("base_asset")?,
            quote_asset: row.try_get("quote_asset")?,
            instrument_type: row.try_get("instrument_type")?,
            min_trade_size: row.try_get("min_trade_size")?,
            min_trade_quantity: row.try_get("min_trade_quantity")?,
            tick_size: row.try_get("tick_size")?,
        })
    }
}

impl BacktestInstTrait for BacktestBinanceInstrumentPo {
    fn inst_id(&self) -> u64 {
        self.inst_id
    }

    fn symbol(&self) -> &str {
        &self.symbol
    }

    fn status(&self) -> &str {
        &self.status
    }

    fn base_asset(&self) -> &str {
        &self.base_asset
    }

    fn quote_asset(&self) -> &str {
        &self.quote_asset
    }

    fn instrument_type(&self) -> &str {
        &self.instrument_type
    }

    fn min_trade_size(&self) -> Decimal {
        self.min_trade_size
    }

    fn min_trade_quantity(&self) -> Decimal {
        self.min_trade_quantity
    }

    fn tick_size(&self) -> Decimal {
        self.tick_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 测试目的：验证 Binance 回测 PO 实现 BacktestInstTrait 后能正确提供全部回测字段。
    // 准备数据/过程：构造包含交易对标识、资产信息和交易过滤值的 PO，并逐项调用 trait 方法。
    // 预期结果：每个方法均返回 PO 中对应的字段值。
    #[test]
    fn backtest_instrument_trait_returns_po_fields() {
        let instrument = BacktestBinanceInstrumentPo {
            inst_id: 42,
            symbol: "BTCUSDT".to_string(),
            status: "TRADING".to_string(),
            base_asset: "BTC".to_string(),
            quote_asset: "USDT".to_string(),
            instrument_type: "binance_SPOT".to_string(),
            min_trade_size: Decimal::new(5, 0),
            min_trade_quantity: Decimal::new(1, 3),
            tick_size: Decimal::new(1, 2),
        };

        assert_eq!(instrument.inst_id(), 42);
        assert_eq!(instrument.symbol(), "BTCUSDT");
        assert_eq!(instrument.status(), "TRADING");
        assert_eq!(instrument.base_asset(), "BTC");
        assert_eq!(instrument.quote_asset(), "USDT");
        assert_eq!(instrument.instrument_type(), "binance_SPOT");
        assert_eq!(instrument.min_trade_size(), Decimal::new(5, 0));
        assert_eq!(instrument.min_trade_quantity(), Decimal::new(1, 3));
        assert_eq!(instrument.tick_size(), Decimal::new(1, 2));
    }
}
