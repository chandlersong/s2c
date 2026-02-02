use std::time::Duration;

use actix::{Actor, AsyncContext, Context, Handler, Supervised};
use log::{debug, error, info};
use yue::binance::bn_models::spot_websocket::{AccountWebSocketPayLoad, BinanceSpotWebSocketResponse, ExecutionReportPayload};

use crate::binance::models::po::SpotOrderPo;
use crate::duck_db::DBProvider;
use crate::errors::YuError;
use duckdb::params;

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

    fn handle_execution_report(&mut self, payload: AccountWebSocketPayLoad<ExecutionReportPayload>) {
        let event_payload = &payload.event;
        let mut order_po = SpotOrderPo::from(event_payload.clone());
        order_po.symbol = order_po.symbol.to_ascii_uppercase();

        self.order_buffer.push(order_po);

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
        let mut stmt = conn.prepare(
            "INSERT OR REPLACE INTO bn_order_events_spot (
                event, event_time, symbol, client_order_id, side, order_type,
                time_in_force, order_qty, order_price, stop_price, iceberg_qty, order_list_id,
                original_client_order_id, execution_type, order_status, reject_reason,
                order_id, last_executed_qty, cumulative_filled_qty, last_executed_price,
                commission_amount, commission_asset, trade_time, trade_id, stp,
                order_creation_time, is_working, is_maker, is_best_match,
                order_create_time, cumulative_quote_qty, last_quote_qty,
                quote_order_quantity, working_time, self_trade_prevention_mode,
                trailing_delta, trailing_time, strategy_id, strategy_type,
                prevented_quantity, last_prevented_quantity, trade_group_id,
                counter_order_id, counter_symbol, prevented_execution_quantity,
                prevented_execution_price, prevented_execution_quote_qty,
                match_type, allocation_id, working_floor, used_sor,
                pegged_price_type, pegged_offset_type, pegged_offset_value, pegged_price
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )?;

        for record in &self.order_buffer {
            stmt.execute(params![
                &record.event,
                record.event_time,
                &record.symbol,
                &record.client_order_id,
                &record.side,
                &record.order_type,
                &record.time_in_force,
                record.order_qty,
                record.order_price,
                record.stop_price,
                record.iceberg_qty,
                record.order_list_id,
                &record.original_client_order_id,
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
                record.stp,
                record.order_creation_time,
                record.is_working,
                record.is_maker,
                record.is_best_match,
                record.order_create_time,
                record.cumulative_quote_qty,
                record.last_quote_qty,
                record.quote_order_quantity,
                record.working_time,
                &record.self_trade_prevention_mode,
                record.trailing_delta,
                record.trailing_time,
                record.strategy_id,
                record.strategy_type,
                record.prevented_quantity,
                record.last_prevented_quantity,
                record.trade_group_id,
                record.counter_order_id,
                &record.counter_symbol,
                record.prevented_execution_quantity,
                record.prevented_execution_price,
                record.prevented_execution_quote_qty,
                &record.match_type,
                record.allocation_id,
                &record.working_floor,
                record.used_sor,
                &record.pegged_price_type,
                &record.pegged_offset_type,
                record.pegged_offset_value,
                record.pegged_price,
            ])?;
        }
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

impl Handler<BinanceSpotWebSocketResponse> for AccountSyncActor {
    type Result = ();

    fn handle(&mut self, msg: BinanceSpotWebSocketResponse, _ctx: &mut Context<Self>) -> Self::Result {
        self.received_count += 1;
        match msg {
            BinanceSpotWebSocketResponse::ExecutionReport(payload) => self.handle_execution_report(payload),
            other => {
                info!("收到非订单事件消息: {:?}", other);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_memory_db_provider;

    #[test]
    fn test_config_defaults() {
        let cfg = AccountSyncConfig::default();
        assert_eq!(cfg.batch_size, 100);
        assert_eq!(cfg.flush_interval_ms, 1000);
    }

    #[test]
    fn test_execution_report_uppercase_symbol() {
        let db = create_memory_db_provider();
        let mut actor = AccountSyncActor::new(Some(db));

        let payload = AccountWebSocketPayLoad {
            subscription_id: 456,

            account_name: None,
            event: ExecutionReportPayload {
                event: "executionReport".to_string(),
                event_time: 1234567891,
                symbol: "btcusdt".to_string(),
                client_order_id: "order1".to_string(),
                side: "BUY".to_string(),
                order_type: "LIMIT".to_string(),
                time_in_force: "GTC".to_string(),
                order_qty: rust_decimal::Decimal::new(1, 0),
                order_price: rust_decimal::Decimal::new(30000, 0),
                stop_price: rust_decimal::Decimal::new(0, 0),
                iceberg_qty: rust_decimal::Decimal::new(0, 0),
                order_list_id: -1,
                original_client_order_id: "".to_string(),
                execution_type: "TRADE".to_string(),
                order_status: "FILLED".to_string(),
                reject_reason: "NONE".to_string(),
                order_id: 999,
                last_executed_qty: rust_decimal::Decimal::new(1, 0),
                cumulative_filled_qty: rust_decimal::Decimal::new(1, 0),
                last_executed_price: rust_decimal::Decimal::new(30000, 0),
                commission_amount: rust_decimal::Decimal::new(0, 0),
                commission_asset: None,
                trade_time: 1234567891,
                trade_id: Some(1),
                stp: None,
                order_creation_time: 1234567890,
                is_working: true,
                is_maker: false,
                is_best_match: true,
                order_create_time: 1234567890,
                cumulative_quote_qty: rust_decimal::Decimal::new(30000, 0),
                last_quote_qty: rust_decimal::Decimal::new(30000, 0),
                quote_order_quantity: rust_decimal::Decimal::new(0, 0),
                working_time: 1234567890,
                self_trade_prevention_mode: "NONE".to_string(),
                trailing_delta: None,
                trailing_time: None,
                strategy_id: None,
                strategy_type: None,
                prevented_quantity: None,
                last_prevented_quantity: None,
                trade_group_id: None,
                counter_order_id: None,
                counter_symbol: None,
                prevented_execution_quantity: None,
                prevented_execution_price: None,
                prevented_execution_quote_qty: None,
                match_type: None,
                allocation_id: None,
                working_floor: None,
                used_sor: None,
                pegged_price_type: None,
                pegged_offset_type: None,
                pegged_offset_value: None,
                pegged_price: None,
            },
        };

        actor.handle_execution_report(payload);
        assert_eq!(actor.order_buffer.len(), 1);
        assert_eq!(actor.order_buffer[0].symbol, "BTCUSDT");
        assert_eq!(actor.order_buffer[0].order_id, 999);
        assert_eq!(actor.order_buffer[0].time_in_force, "GTC");
        assert_eq!(actor.order_buffer[0].execution_type, "TRADE");
        assert_eq!(actor.order_buffer[0].is_maker, false);
    }
}
