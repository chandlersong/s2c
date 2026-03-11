use std::sync::{LazyLock, OnceLock};
use std::time::Duration;

use actix::{Actor, Addr, AsyncContext, Context, Handler, Supervised};
use log::{debug, error, info};
use yue::binance::bn_models::spot_websocket::ExecutionReportPayload;

use crate::binance::models::po::SpotOrderPo;
use crate::duck_db::{DBProvider, CONNECTION_POOL};
use crate::errors::YuError;
use duckdb::{params, DuckdbConnectionManager};
use r2d2::Pool;
use yue::binance::bn_models::common::{PortfolioSpotOrderData, SpotOrderData};
use yue::binance::bn_restful_commands::{get_bn_funding_rate_limit, SWAP_FUNDING_RATE_PATH};

pub(crate) static BINANCE_ACCOUNT_ACTOR: OnceLock<Addr<AccountSyncActor>> = OnceLock::new();

pub fn get_account_addr() -> Addr<AccountSyncActor> {
    BINANCE_ACCOUNT_ACTOR.get_or_init(|| AccountSyncActor::new(None).start()).clone()
}
/// AccountSyncActor 负责将订单事件批量落库。
pub struct AccountSyncActor {
    db: DBProvider,
    config: AccountSyncConfig,
    order_buffer: Vec<SpotOrderPo>,
    received_count: u64,
    flushed_order: u64,
}

#[derive(Debug, Clone)]
pub struct AccountSyncConfig {
    pub batch_size: usize,
    pub flush_interval_ms: u64,
}

impl Default for AccountSyncConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            flush_interval_ms: 1000,
        }
    }
}

impl AccountSyncActor {
    pub fn new(source_db: Option<DBProvider>) -> Self {
        let cfg = AccountSyncConfig::default();
        let db = source_db.unwrap_or_else(|| DBProvider::default());
        AccountSyncActor {
            db,
            config: cfg,
            order_buffer: Vec::new(),
            received_count: 0,
            flushed_order: 0,
        }
    }

    fn batch_size(&self) -> usize {
        self.config.batch_size
    }

    fn flush_interval(&self) -> Duration {
        Duration::from_millis(self.config.flush_interval_ms)
    }

    fn handle_execution_report(&mut self, mut record: SpotOrderPo) {
        record.symbol = record.symbol.to_ascii_uppercase();

        self.order_buffer.push(record);

        if self.order_buffer.len() >= self.batch_size() {
            self.flush_orders();
        }
    }

    fn flush_orders(&mut self) {
        if self.order_buffer.is_empty() {
            return;
        }
        debug!("Flushing {} order records", self.order_buffer.len());
        match self.write_orders_to_db() {
            Ok(count) => {
                self.flushed_order += count as u64;
                debug!("Flushed orders: {} (total {})", count, self.flushed_order);
            }
            Err(e) => {
                error!("Failed to flush orders: {:?}", e);
            }
        }
        self.order_buffer.clear();
    }

    fn write_orders_to_db(&self) -> Result<usize, YuError> {
        let conn = self.db.acquire()?;
        conn.execute_batch("BEGIN")?;
        let mut stmt = conn.prepare(
            "INSERT OR REPLACE INTO bn_order_events_spot (
                event, account_name, event_time,symbol, client_order_id, side, order_type,
                time_in_force, order_qty, order_price, stop_price, execution_type,
                order_status, reject_reason, order_id, last_executed_qty,
                cumulative_filled_qty, last_executed_price, commission_amount,
                commission_asset, trade_time, trade_id, is_maker, is_working,
                order_create_time, cumulative_quote_qty, last_quote_qty, quote_order_quantity
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )?;

        for record in &self.order_buffer {
            stmt.execute(params![
                &record.event,
                record.account_name,
                record.event_time,
                &record.symbol,
                &record.client_order_id,
                &record.side,
                &record.order_type,
                &record.time_in_force,
                record.order_qty,
                record.order_price,
                record.stop_price,
                &record.execution_type,
                &record.order_status,
                &record.reject_reason,
                record.order_id,
                record.last_executed_qty,
                record.cumulative_filled_qty,
                record.last_executed_price,
                record.commission_amount,
                &record.commission_asset,
                record.trade_time,
                record.trade_id,
                record.is_maker,
                record.is_working,
                record.order_create_time,
                record.cumulative_quote_qty,
                record.last_quote_qty,
                record.quote_order_quantity,
            ])?;
        }

        conn.execute_batch("COMMIT")?;
        Ok(self.order_buffer.len())
    }
}

impl Actor for AccountSyncActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.set_mailbox_capacity(2000);
        let interval = self.flush_interval();
        ctx.run_interval(interval, |act, _ctx| {
            act.flush_orders();
        });
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!(
            "AccountSyncActor for stopped. received={}, flushed_order={}",
            self.received_count, self.flushed_order
        );
        self.flush_orders();
    }
}

impl Supervised for AccountSyncActor {}

impl Handler<SpotOrderData> for AccountSyncActor {
    type Result = ();

    fn handle(&mut self, msg: SpotOrderData, _ctx: &mut Context<Self>) -> Self::Result {
        self.received_count += 1;
        let record = SpotOrderPo::from(msg);
        self.handle_execution_report(record)
    }
}

impl Handler<PortfolioSpotOrderData> for AccountSyncActor {
    type Result = ();

    fn handle(&mut self, msg: PortfolioSpotOrderData, _ctx: &mut Context<Self>) -> Self::Result {
        self.received_count += 1;
        let record = SpotOrderPo::from(msg);
        self.handle_execution_report(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let cfg = AccountSyncConfig::default();
        assert_eq!(cfg.batch_size, 100);
        assert_eq!(cfg.flush_interval_ms, 1000);
    }
}
