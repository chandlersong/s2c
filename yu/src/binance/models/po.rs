use crate::binance::history_task::HistoryPO;
use crate::utils::get_snowflake_generator;
use duckdb::appender_params_from_iter;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_models::spot_websocket::ExecutionReportPayload;
use yue::binance::bn_models::spot_websocket_stream::{KlineData, TradeStreamPayload};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotStreamTradeRecordPo {
    pub id: i64,
    pub event_time: i64,
    pub symbol: String,
    pub trade_id: i64,
    pub price: f64,
    pub qty: f64,
    pub trade_time: Option<i64>,
    pub is_buyer_maker: Option<bool>,
    pub created_at: i64,
}

impl From<TradeStreamPayload> for SpotStreamTradeRecordPo {
    fn from(payload: TradeStreamPayload) -> Self {
        SpotStreamTradeRecordPo {
            id: get_snowflake_generator().lock().unwrap().real_time_generate(),
            event_time: payload.event_time as i64,
            symbol: payload.symbol,
            trade_id: payload.trade_id as i64,
            price: payload.price.to_f64().unwrap(),
            qty: payload.qty.to_f64().unwrap(),
            trade_time: Some(payload.trade_time as i64),
            is_buyer_maker: Some(payload.is_buyer_maker),
            created_at: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotOrderPo {
    pub event: String,
    pub event_time: i64,
    pub symbol: String,
    pub client_order_id: String,
    pub side: String,
    pub order_type: String,
    pub time_in_force: String,
    pub order_qty: f64,
    pub order_price: f64,
    pub stop_price: f64,
    pub iceberg_qty: f64,
    pub order_list_id: i64,
    pub original_client_order_id: String,
    pub execution_type: String,
    pub order_status: String,
    pub reject_reason: String,
    pub order_id: i64,
    pub last_executed_qty: f64,
    pub cumulative_filled_qty: f64,
    pub last_executed_price: f64,
    pub commission_amount: f64,
    pub commission_asset: Option<String>,
    pub trade_time: i64,
    pub trade_id: Option<i64>,
    pub stp: Option<i64>,
    pub order_creation_time: i64,
    pub is_working: bool,
    pub is_maker: bool,
    pub is_best_match: bool,
    pub order_create_time: i64,
    pub cumulative_quote_qty: f64,
    pub last_quote_qty: f64,
    pub quote_order_quantity: f64,
    pub working_time: i64,
    pub self_trade_prevention_mode: String,
    pub trailing_delta: Option<f64>,
    pub trailing_time: Option<i64>,
    pub strategy_id: Option<u64>,
    pub strategy_type: Option<u64>,
    pub prevented_quantity: Option<f64>,
    pub last_prevented_quantity: Option<f64>,
    pub trade_group_id: Option<u64>,
    pub counter_order_id: Option<bool>,
    pub counter_symbol: Option<String>,
    pub prevented_execution_quantity: Option<f64>,
    pub prevented_execution_price: Option<f64>,
    pub prevented_execution_quote_qty: Option<f64>,
    pub match_type: Option<String>,
    pub allocation_id: Option<u64>,
    pub working_floor: Option<String>,
    pub used_sor: Option<bool>,
    pub pegged_price_type: Option<String>,
    pub pegged_offset_type: Option<String>,
    pub pegged_offset_value: Option<u64>,
    pub pegged_price: Option<f64>,
}

impl From<ExecutionReportPayload> for SpotOrderPo {
    fn from(payload: ExecutionReportPayload) -> Self {
        SpotOrderPo {
            event: payload.event,
            event_time: payload.event_time as i64,
            symbol: payload.symbol,
            client_order_id: payload.client_order_id,
            side: payload.side,
            order_type: payload.order_type,
            time_in_force: payload.time_in_force,
            order_qty: payload.order_qty.to_f64().unwrap_or(0.0),
            order_price: payload.order_price.to_f64().unwrap_or(0.0),
            stop_price: payload.stop_price.to_f64().unwrap_or(0.0),
            iceberg_qty: payload.iceberg_qty.to_f64().unwrap_or(0.0),
            order_list_id: payload.order_list_id,
            original_client_order_id: payload.original_client_order_id,
            execution_type: payload.execution_type,
            order_status: payload.order_status,
            reject_reason: payload.reject_reason,
            order_id: payload.order_id,
            last_executed_qty: payload.last_executed_qty.to_f64().unwrap_or(0.0),
            cumulative_filled_qty: payload.cumulative_filled_qty.to_f64().unwrap_or(0.0),
            last_executed_price: payload.last_executed_price.to_f64().unwrap_or(0.0),
            commission_amount: payload.commission_amount.to_f64().unwrap_or(0.0),
            commission_asset: payload.commission_asset,
            trade_time: payload.trade_time as i64,
            trade_id: payload.trade_id,
            stp: payload.stp,
            order_creation_time: payload.order_creation_time as i64,
            is_working: payload.is_working,
            is_maker: payload.is_maker,
            is_best_match: payload.is_best_match,
            order_create_time: payload.order_create_time as i64,
            cumulative_quote_qty: payload.cumulative_quote_qty.to_f64().unwrap_or(0.0),
            last_quote_qty: payload.last_quote_qty.to_f64().unwrap_or(0.0),
            quote_order_quantity: payload.quote_order_quantity.to_f64().unwrap_or(0.0),
            working_time: payload.working_time as i64,
            self_trade_prevention_mode: payload.self_trade_prevention_mode,
            trailing_delta: payload.trailing_delta.map(|d| d.to_f64().unwrap_or(0.0)),
            trailing_time: payload.trailing_time.map(|t| t as i64),
            strategy_id: payload.strategy_id,
            strategy_type: payload.strategy_type,
            prevented_quantity: payload.prevented_quantity.map(|p| p.to_f64().unwrap_or(0.0)),
            last_prevented_quantity: payload.last_prevented_quantity.map(|p| p.to_f64().unwrap_or(0.0)),
            trade_group_id: payload.trade_group_id,
            counter_order_id: payload.counter_order_id,
            counter_symbol: payload.counter_symbol,
            prevented_execution_quantity: payload.prevented_execution_quantity.map(|p| p.to_f64().unwrap_or(0.0)),
            prevented_execution_price: payload.prevented_execution_price.map(|p| p.to_f64().unwrap_or(0.0)),
            prevented_execution_quote_qty: payload.prevented_execution_quote_qty.map(|p| p.to_f64().unwrap_or(0.0)),
            match_type: payload.match_type,
            allocation_id: payload.allocation_id,
            working_floor: payload.working_floor,
            used_sor: payload.used_sor,
            pegged_price_type: payload.pegged_price_type,
            pegged_offset_type: payload.pegged_offset_type,
            pegged_offset_value: payload.pegged_offset_value,
            pegged_price: payload.pegged_price.map(|p| p.to_f64().unwrap_or(0.0)),
        }
    }
}

pub const INTERVAL_5M: u8 = 1;

#[derive(Debug, Clone)]
pub struct KlinePo {
    pub id: i64,
    pub symbol: String,
    pub candle_begin_time: u64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub quote_volume: f64,
    pub number_of_trades: u64,
    pub taker_buy_base_asset_volume: f64,
    pub taker_buy_quote_asset_volume: f64,
    pub close_time: u64,
    /// K 线周期（如 "1m"）。
    pub interval: u8, //因为节省数据，所以换成了u8，现在只存5m。所以为0
    /// 第一笔成交 ID。
    pub first_trade_id: Option<i64>,
    /// 最后一笔成交 ID。
    pub last_trade_id: Option<i64>,
}

impl From<KlineData> for KlinePo {
    fn from(value: KlineData) -> Self {
        KlinePo {
            id: get_snowflake_generator().lock().unwrap().real_time_generate(),
            symbol: value.symbol,
            candle_begin_time: value.start_time,
            open: value.open.to_f64().unwrap(),
            high: value.high.to_f64().unwrap(),
            low: value.low.to_f64().unwrap(),
            close: value.close.to_f64().unwrap(),
            volume: value.volume.to_f64().unwrap(),
            quote_volume: value.quote_volume.to_f64().unwrap(),
            number_of_trades: value.trade_count,
            taker_buy_base_asset_volume: value.taker_buy_base_volume.to_f64().unwrap(),
            taker_buy_quote_asset_volume: value.taker_buy_quote_volume.to_f64().unwrap(),
            close_time: value.close_time,
            interval: INTERVAL_5M,
            first_trade_id: Some(value.first_trade_id),
            last_trade_id: Some(value.last_trade_id),
        }
    }
}

impl<'a> From<&duckdb::Row<'a>> for KlinePo {
    fn from(row: &duckdb::Row) -> Self {
        KlinePo {
            id: row.get(0).unwrap_or_default(),
            symbol: row.get(1).unwrap_or_default(),
            candle_begin_time: row.get(2).unwrap_or_default(),
            open: row.get(3).unwrap_or_default(),
            high: row.get(4).unwrap_or_default(),
            low: row.get(5).unwrap_or_default(),
            close: row.get(6).unwrap_or_default(),
            volume: row.get(7).unwrap_or_default(),
            quote_volume: row.get(8).unwrap_or_default(),
            number_of_trades: row.get(9).unwrap_or_default(),
            taker_buy_base_asset_volume: row.get(10).unwrap_or_default(),
            taker_buy_quote_asset_volume: row.get(11).unwrap_or_default(),
            close_time: row.get(12).unwrap_or_default(),
            interval: row.get(13).unwrap_or_default(),
            first_trade_id: row.get(14).unwrap_or_default(),
            last_trade_id: row.get(15).unwrap_or_default(),
        }
    }
}

impl HistoryPO for KlinePo {
    type Source = BinanceKline;

    fn from_source(symbol: Option<&str>, source: &Self::Source) -> Self {
        let id = get_snowflake_generator().lock().unwrap().real_time_generate();
        KlinePo {
            id,
            symbol: symbol.expect("Symbol must be provided").to_string(),
            candle_begin_time: source.open_time,
            open: source.open.to_f64().unwrap(),
            high: source.high.to_f64().unwrap(),
            low: source.low.to_f64().unwrap(),
            close: source.close.to_f64().unwrap(),
            volume: source.volume.to_f64().unwrap(),
            quote_volume: source.quote_asset_volume.to_f64().unwrap(),
            number_of_trades: source.number_of_trades,
            taker_buy_base_asset_volume: source.taker_buy_base_asset_volume.to_f64().unwrap(),
            taker_buy_quote_asset_volume: source.taker_buy_quote_asset_volume.to_f64().unwrap(),
            close_time: source.close_time,
            interval: 0,
            first_trade_id: None,
            last_trade_id: None,
        }
    }

    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>> {
        appender_params_from_iter(vec![
            &self.id as &dyn duckdb::ToSql,
            &self.symbol as &dyn duckdb::ToSql,
            &self.candle_begin_time as &dyn duckdb::ToSql,
            &self.open as &dyn duckdb::ToSql,
            &self.high as &dyn duckdb::ToSql,
            &self.low as &dyn duckdb::ToSql,
            &self.close as &dyn duckdb::ToSql,
            &self.volume as &dyn duckdb::ToSql,
            &self.quote_volume as &dyn duckdb::ToSql,
            &self.number_of_trades as &dyn duckdb::ToSql,
            &self.taker_buy_base_asset_volume as &dyn duckdb::ToSql,
            &self.taker_buy_quote_asset_volume as &dyn duckdb::ToSql,
            &self.close_time as &dyn duckdb::ToSql,
            &self.interval as &dyn duckdb::ToSql,
            &self.first_trade_id as &dyn duckdb::ToSql,
            &self.last_trade_id as &dyn duckdb::ToSql,
        ])
    }
}

impl Display for KlinePo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "KlineData {{ id: {},interval {}, symbol: {}, candle_begin_time: {}, open: {}, high: {}, low: {}, close: {}, volume: {}, quote_volume: {}, number_of_trades: {}, taker_buy_base_asset_volume: {}, taker_buy_quote_asset_volume: {}, close_time: {} }}",
            self.id,
            self.interval,
            self.symbol,
            self.candle_begin_time,
            self.open,
            self.high,
            self.low,
            self.close,
            self.volume,
            self.quote_volume,
            self.number_of_trades,
            self.taker_buy_base_asset_volume,
            self.taker_buy_quote_asset_volume,
            self.close_time
        )
    }
}
