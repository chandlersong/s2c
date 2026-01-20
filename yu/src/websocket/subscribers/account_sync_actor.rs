use std::time::Duration;

use actix::{Actor, AsyncContext, Context, Handler, Supervised};
use log::{debug, error, info};
use rust_decimal::prelude::ToPrimitive;
use yue::binance::bn_models::spot_websocket::{
    AccountWebSocketPayLoad, BinanceSpotWebSocketResponse, ExecutionReportPayload, OutboundAccountPositionPayload,
};

use crate::binance::models::ws_db_po::{SpotBalancePo, SpotOrderPo};
use crate::duck_db::DBProvider;
use crate::errors::YuError;
use crate::websocket::binance_spot::init_tables::create_spot_websocket_tables;
use duckdb::params;

/// AccountSyncActor 负责将账户余额与订单事件批量落库。
pub struct AccountSyncActor {
    account_id: String,
    db: DBProvider,
    config: AccountSyncConfig,
    balance_buffer: Vec<SpotBalancePo>,
    order_buffer: Vec<SpotOrderPo>,
    received_count: u64,
    flushed_balance: u64,
    flushed_order: u64,
}

#[derive(Debug, Clone)]
pub struct AccountSyncConfig {
    pub batch_size: usize,
    pub flush_interval_ms: u64,
    pub source_exchange: &'static str,
}

impl Default for AccountSyncConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            flush_interval_ms: 1000,
            source_exchange: "BINANCE",
        }
    }
}

impl AccountSyncActor {
    pub fn new(account_id: impl Into<String>, db: DBProvider, config: Option<AccountSyncConfig>) -> Self {
        let cfg = config.unwrap_or_default();
        AccountSyncActor {
            account_id: account_id.into(),
            db,
            config: cfg,
            balance_buffer: Vec::new(),
            order_buffer: Vec::new(),
            received_count: 0,
            flushed_balance: 0,
            flushed_order: 0,
        }
    }

    fn batch_size(&self) -> usize {
        self.config.batch_size
    }

    fn flush_interval(&self) -> Duration {
        Duration::from_millis(self.config.flush_interval_ms)
    }

    fn handle_outbound(&mut self, payload: AccountWebSocketPayLoad<OutboundAccountPositionPayload>) {
        let event_payload = &payload.event;
        let raw_json = serde_json::to_string(&event_payload).unwrap_or_default();
        for b in &event_payload.balances {
            self.balance_buffer.push(SpotBalancePo {
                account_id: self.account_id.clone(),
                asset: b.asset.clone(),
                free: b.free.to_f64().unwrap_or(0.0),
                locked: b.locked.to_f64().unwrap_or(0.0),
                event_time: event_payload.event_time as i64,
                source_exchange: self.config.source_exchange.to_string(),
                raw_json: raw_json.clone(),
            });
        }

        if self.balance_buffer.len() >= self.batch_size() {
            self.flush_balances();
        }
    }

    fn handle_balance_update(&mut self, payload: AccountWebSocketPayLoad<yue::binance::bn_models::spot_websocket::BalanceUpdatePayload>) {
        let event_payload = &payload.event;
        let raw_json = serde_json::to_string(&event_payload).unwrap_or_default();

        // balanceUpdate只包含单个资产的变动，我们直接记录
        // 注意：这里只记录变动的资产，不是完整的余额状态
        self.balance_buffer.push(SpotBalancePo {
            account_id: self.account_id.clone(),
            asset: event_payload.asset.clone(),
            free: event_payload.balance_delta.to_f64().unwrap_or(0.0), // 这是变动量
            locked: 0.0,                                               // balanceUpdate没有locked字段
            event_time: event_payload.event_time as i64,
            source_exchange: self.config.source_exchange.to_string(),
            raw_json,
        });

        if self.balance_buffer.len() >= self.batch_size() {
            self.flush_balances();
        }
    }
    fn handle_execution_report(&mut self, payload: AccountWebSocketPayLoad<ExecutionReportPayload>) {
        let event_payload = &payload.event;
        let raw_json = serde_json::to_string(&event_payload).unwrap_or_default();
        self.order_buffer.push(SpotOrderPo {
            account_id: self.account_id.clone(),
            symbol: event_payload.symbol.to_ascii_uppercase(),
            order_id: event_payload.order_id.to_string(),
            client_order_id: event_payload.client_order_id.clone(),
            status: event_payload.order_status.clone(),
            side: event_payload.side.clone(),
            order_type: event_payload.order_type.clone(),
            price: event_payload.order_price.to_f64().unwrap_or(0.0),
            qty: event_payload.order_qty.to_f64().unwrap_or(0.0),
            exec_qty: event_payload.cumulative_filled_qty.to_f64().unwrap_or(0.0),
            last_exec_price: event_payload.last_executed_price.to_f64().unwrap_or(0.0),
            event_time: event_payload.event_time as i64,
            source_exchange: self.config.source_exchange.to_string(),
            raw_json,
        });

        if self.order_buffer.len() >= self.batch_size() {
            self.flush_orders();
        }
    }

    fn flush_balances(&mut self) {
        if self.balance_buffer.is_empty() {
            return;
        }
        debug!("Flushing {} balance records", self.balance_buffer.len());
        match self.write_balances_to_db() {
            Ok(count) => {
                self.flushed_balance += count as u64;
                debug!("Flushed balances: {} (total {})", count, self.flushed_balance);
            }
            Err(e) => {
                error!("Failed to flush balances: {:?}", e);
            }
        }
        self.balance_buffer.clear();
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

    fn write_balances_to_db(&self) -> Result<usize, YuError> {
        let conn = self.db.acquire()?;
        create_spot_websocket_tables(Some(&conn))?;
        let mut stmt = conn.prepare(
            "INSERT OR REPLACE INTO account_balance_spot (account_id, asset, free, locked, event_time, source_exchange, raw_json) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )?;

        for record in &self.balance_buffer {
            stmt.execute(params![
                record.account_id,
                record.asset,
                record.free,
                record.locked,
                record.event_time,
                &record.source_exchange,
                &record.raw_json,
            ])?;
        }
        Ok(self.balance_buffer.len())
    }

    fn write_orders_to_db(&self) -> Result<usize, YuError> {
        let conn = self.db.acquire()?;
        create_spot_websocket_tables(Some(&conn))?;
        let mut stmt = conn.prepare(
            "INSERT OR REPLACE INTO order_events_spot (account_id, symbol, order_id, client_order_id, status, side, type, price, qty, exec_qty, last_exec_price, event_time, source_exchange, raw_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )?;

        for record in &self.order_buffer {
            stmt.execute(params![
                record.account_id,
                record.symbol.clone(),
                record.order_id.clone(),
                record.client_order_id,
                record.status.clone(),
                record.side.clone(),
                record.order_type.clone(),
                record.price,
                record.qty,
                record.exec_qty,
                record.last_exec_price,
                record.event_time,
                &record.source_exchange,
                &record.raw_json,
            ])?;
        }
        Ok(self.order_buffer.len())
    }
}

impl Actor for AccountSyncActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.set_mailbox_capacity(2000);
        info!("AccountSyncActor for {} started", self.account_id);
        let interval = self.flush_interval();
        ctx.run_interval(interval, |act, _ctx| {
            act.flush_balances();
            act.flush_orders();
        });
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!(
            "AccountSyncActor for {} stopped. received={}, flushed_balance={}, flushed_order={}",
            self.account_id, self.received_count, self.flushed_balance, self.flushed_order
        );
        self.flush_balances();
        self.flush_orders();
    }
}

impl Supervised for AccountSyncActor {}

impl Handler<BinanceSpotWebSocketResponse> for AccountSyncActor {
    type Result = ();

    fn handle(&mut self, msg: BinanceSpotWebSocketResponse, _ctx: &mut Context<Self>) -> Self::Result {
        self.received_count += 1;
        match msg {
            BinanceSpotWebSocketResponse::OutboundAccountPosition(payload) => self.handle_outbound(payload),
            BinanceSpotWebSocketResponse::BalanceUpdate(payload) => self.handle_balance_update(payload),
            BinanceSpotWebSocketResponse::ExecutionReport(payload) => self.handle_execution_report(payload),
            BinanceSpotWebSocketResponse::SubscribeResponse(_) => {
                // 订阅响应不落库，记录收到次数即可
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use duckdb::DuckdbConnectionManager;
    use r2d2::Pool;

    fn create_memory_db() -> DBProvider {
        let manager = DuckdbConnectionManager::memory().unwrap();
        let pool = Pool::builder().max_size(4).build(manager).unwrap();
        DBProvider::new(pool)
    }

    #[test]
    fn test_config_defaults() {
        let cfg = AccountSyncConfig::default();
        assert_eq!(cfg.batch_size, 100);
        assert_eq!(cfg.flush_interval_ms, 1000);
        assert_eq!(cfg.source_exchange, "BINANCE");
    }

    #[test]
    fn test_actor_creation() {
        let cfg = AccountSyncConfig {
            batch_size: 50,
            flush_interval_ms: 500,
            ..Default::default()
        };
        let db = create_memory_db();
        let actor = AccountSyncActor::new("test_acc", db, Some(cfg.clone()));
        assert_eq!(actor.account_id, "test_acc");
        assert_eq!(actor.batch_size(), 50);
    }

    #[test]
    fn test_balance_buffer_empty_by_default() {
        let db = create_memory_db();
        let actor = AccountSyncActor::new("test", db, None);
        assert_eq!(actor.balance_buffer.len(), 0);
        assert_eq!(actor.order_buffer.len(), 0);
    }

    #[test]
    fn test_outbound_handler_creates_buffer() {
        let db = create_memory_db();
        let mut actor = AccountSyncActor::new(
            "test",
            db,
            Some(AccountSyncConfig {
                batch_size: 10,
                ..Default::default()
            }),
        );

        let payload = AccountWebSocketPayLoad {
            subscription_id: 123,
            account_name: None,
            event: OutboundAccountPositionPayload {
                event: "outboundAccountPosition".to_string(),
                event_time: 1234567890,
                last_account_update: 1234567890,
                balances: vec![
                    yue::binance::bn_models::spot_websocket::BalanceItem {
                        asset: "BTC".to_string(),
                        free: rust_decimal::Decimal::new(1, 0),
                        locked: rust_decimal::Decimal::new(0, 0),
                    },
                    yue::binance::bn_models::spot_websocket::BalanceItem {
                        asset: "USDT".to_string(),
                        free: rust_decimal::Decimal::new(10000, 0),
                        locked: rust_decimal::Decimal::new(500, 0),
                    },
                ],
            },
        };

        actor.handle_outbound(payload);
        assert_eq!(actor.balance_buffer.len(), 2);
        assert_eq!(actor.balance_buffer[0].asset, "BTC");
        assert_eq!(actor.balance_buffer[1].asset, "USDT");
        assert_eq!(actor.balance_buffer[0].account_id, "test");
    }

    #[test]
    fn test_execution_report_uppercase_symbol() {
        let db = create_memory_db();
        let mut actor = AccountSyncActor::new(
            "test2",
            db,
            Some(AccountSyncConfig {
                batch_size: 10,
                ..Default::default()
            }),
        );

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
        assert_eq!(actor.order_buffer[0].order_id, "999");
        assert_eq!(actor.order_buffer[0].account_id, "test2");
    }
}
