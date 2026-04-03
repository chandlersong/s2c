use duckdb::appender_params_from_iter;
use rust_decimal::prelude::ToPrimitive;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};
use yue::binance::bn_models::common::{HistoryVo, PortfolioSpotOrderData, PortfolioSwapOrderData, SpotOrderData, SwapOrderData};
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_models::spot_websocket_stream::{KlineData, TradeStreamPayload};
use yue::binance::bn_models::swap_restful::FundingRate;
use yue::tools::{get_snow_flake_id_u64, SnowyFlakeWrapper};

pub trait DuckDBPO: Debug + Clone + DeserializeOwned + 'static + Send + Sync {
    type Source: HistoryVo;

    fn from_source(symbol: Option<&str>, source: &Self::Source) -> Self;

    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>>;
}

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
            id: get_snow_flake_id_u64() as i64,
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

pub const INTERVAL_5M: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
            id: get_snow_flake_id_u64() as i64,
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

impl DuckDBPO for KlinePo {
    type Source = BinanceKline;

    fn from_source(symbol: Option<&str>, source: &Self::Source) -> Self {
        KlinePo {
            id: get_snow_flake_id_u64() as i64,
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
            first_trade_id: Some(-1),
            last_trade_id: Some(-1),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotOrderPo {
    pub event: String,
    pub account_name: String,
    pub event_time: i64,
    pub symbol: String,
    pub client_order_id: Option<String>,
    pub side: String,
    pub order_type: String,
    pub time_in_force: String,
    pub order_qty: f64,
    pub order_price: f64,
    pub stop_price: Option<f64>,
    pub execution_type: String,
    pub order_status: String,
    pub reject_reason: Option<String>,
    pub order_id: i64,
    pub last_executed_qty: Option<f64>,
    pub cumulative_filled_qty: Option<f64>,
    pub last_executed_price: Option<f64>,
    pub commission_amount: Option<f64>,
    pub commission_asset: Option<String>,
    pub trade_time: i64,
    pub trade_id: i64,
    pub is_maker: bool,
    pub is_working: bool,
    pub order_create_time: i64,
    pub cumulative_quote_qty: f64,
    pub last_quote_qty: Option<f64>,
    pub quote_order_quantity: Option<f64>,
}

impl From<SpotOrderData> for SpotOrderPo {
    fn from(data: SpotOrderData) -> Self {
        let payload = data.data;

        Self {
            event: payload.event,
            account_name: data.account_name,
            event_time: payload.event_time as i64,
            symbol: payload.symbol,
            client_order_id: payload.client_order_id,
            side: payload.side,
            order_type: payload.order_type,
            time_in_force: payload.time_in_force,
            order_qty: payload.order_qty.to_f64().unwrap_or(0.0),
            order_price: payload.order_price.to_f64().unwrap_or(0.0),
            stop_price: payload.stop_price.and_then(|d| d.to_f64()),
            execution_type: payload.execution_type,
            order_status: payload.order_status,
            reject_reason: payload.reject_reason,
            order_id: payload.order_id,
            last_executed_qty: payload.last_executed_qty.and_then(|d| d.to_f64()),
            cumulative_filled_qty: payload.cumulative_filled_qty.and_then(|d| d.to_f64()),
            last_executed_price: payload.last_executed_price.and_then(|d| d.to_f64()),
            commission_amount: payload.commission_amount.and_then(|d| d.to_f64()),
            commission_asset: payload.commission_asset,
            trade_time: payload.trade_time as i64,
            trade_id: payload.trade_id,
            is_maker: payload.is_maker,
            is_working: payload.is_working,
            order_create_time: payload.order_create_time as i64,
            cumulative_quote_qty: payload.cumulative_quote_qty.to_f64().unwrap_or(0.0),
            last_quote_qty: payload.last_quote_qty.and_then(|d| d.to_f64()),
            quote_order_quantity: payload.quote_order_quantity.and_then(|d| d.to_f64()),
        }
    }
}

impl From<PortfolioSpotOrderData> for SpotOrderPo {
    fn from(data: PortfolioSpotOrderData) -> Self {
        let payload = data.data;
        Self {
            event: payload.event,
            account_name: data.account_name,
            event_time: payload.event_time as i64,
            symbol: payload.symbol,
            client_order_id: payload.client_order_id,
            side: payload.side,
            order_type: payload.order_type,
            time_in_force: payload.time_in_force,
            order_qty: payload.quantity.to_f64().unwrap_or(0.0),
            order_price: payload.price.to_f64().unwrap_or(0.0),
            stop_price: payload.stop_price.and_then(|d| d.to_f64()),
            execution_type: payload.execution_type,
            order_status: payload.order_status,
            reject_reason: payload.reject_reason,
            order_id: payload.order_id,
            last_executed_qty: payload.last_executed_qty.and_then(|d| d.to_f64()),
            cumulative_filled_qty: payload.cumulative_filled_qty.and_then(|d| d.to_f64()),
            last_executed_price: payload.last_executed_price.and_then(|d| d.to_f64()),
            commission_amount: payload.commission_amount.and_then(|d| d.to_f64()),
            commission_asset: payload.commission_asset,
            trade_time: payload.trade_time as i64,
            trade_id: payload.trade_id,
            is_maker: payload.is_maker,
            is_working: payload.is_working,
            order_create_time: payload.order_create_time as i64,
            cumulative_quote_qty: payload.cumulative_quote_qty.to_f64().unwrap_or(0.0),
            last_quote_qty: payload.last_quote_qty.and_then(|d| d.to_f64()),
            quote_order_quantity: payload.quote_order_quantity.and_then(|d| d.to_f64()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapOrderPo {
    pub event: String,
    pub account_name: String,
    pub event_time: i64,
    pub trade_time: i64,
    pub symbol: String,
    pub client_order_id: Option<String>,
    pub side: String,
    pub order_type: String,
    pub time_in_force: String,
    pub order_qty: f64,
    pub order_price: f64,
    pub avg_price: Option<f64>,
    pub stop_price: Option<f64>,
    pub execution_type: String,
    pub order_status: String,
    pub order_id: i64,
    pub last_filled_qty: Option<f64>,
    pub executed_qty: Option<f64>,
    pub last_filled_price: f64,
    pub commission_asset: Option<String>,
    pub commission_amount: f64,
    pub trade_id: Option<i64>,
    pub is_maker: Option<bool>,
    pub is_reduce_only: bool,
    pub position_side: String,
    pub realized_pnl: f64,
    pub stp_mode: String,
    pub gtd: Option<i64>,
}

impl From<SwapOrderData> for SwapOrderPo {
    fn from(data: SwapOrderData) -> Self {
        let payload = data.data;
        let o = payload.order.expect("Order data must be present");
        Self {
            event: payload.event,
            account_name: data.account_name,
            event_time: payload.event_time as i64,
            trade_time: payload.trade_time as i64,
            symbol: o.symbol,
            client_order_id: o.client_order_id,
            side: o.side.unwrap_or_default(),
            order_type: o.order_type,
            time_in_force: o.time_in_force,
            order_qty: o.quantity.to_f64().unwrap_or(0.0),
            order_price: o.price.to_f64().unwrap_or(0.0),
            avg_price: Some(o.avg_price.to_f64().unwrap_or(0.0)),
            stop_price: Some(o.stop_price.to_f64().unwrap_or(0.0)),
            execution_type: o.execution_type.unwrap_or_default(),
            order_status: o.current_order_status.unwrap_or_default(),
            order_id: o.order_id.unwrap_or_default() as i64,
            last_filled_qty: Some(o.last_filled_qty.to_f64().unwrap_or(0.0)),
            executed_qty: Some(o.executed_qty.to_f64().unwrap_or(0.0)),
            last_filled_price: o.last_filled_price.to_f64().unwrap_or(0.0),
            commission_asset: Some(o.fee_asset),
            commission_amount: o.fee.to_f64().unwrap_or(0.0),
            trade_id: Some(o.trade_id as i64),
            is_maker: Some(o.is_maker),
            is_reduce_only: o.is_reduce_only,
            position_side: o.position_side,
            realized_pnl: o.realized_pnl.to_f64().unwrap_or(0.0),
            stp_mode: o.self_trade_prevention_mode.unwrap_or_default(),
            gtd: o.gtd.map(|v| v as i64),
        }
    }
}

impl From<PortfolioSwapOrderData> for SwapOrderPo {
    fn from(data: PortfolioSwapOrderData) -> Self {
        let payload = data.data;
        let o = payload.order;
        Self {
            event: payload.event,
            account_name: data.account_name,
            event_time: payload.event_time as i64,
            trade_time: payload.trade_time as i64,
            symbol: o.symbol,
            client_order_id: o.client_order_id,
            side: o.side,
            order_type: o.order_type,
            time_in_force: o.time_in_force,
            order_qty: o.quantity.to_f64().unwrap_or(0.0),
            order_price: o.price.to_f64().unwrap_or(0.0),
            avg_price: o.avg_price.and_then(|v| v.to_f64()),
            stop_price: o.stop_price.and_then(|v| v.to_f64()),
            execution_type: o.execution_type,
            order_status: o.current_order_status,
            order_id: o.order_id as i64,
            last_filled_qty: o.last_filled_qty.and_then(|v| v.to_f64()),
            executed_qty: o.executed_qty.and_then(|v| v.to_f64()),
            last_filled_price: o.last_filled_price.to_f64().unwrap_or(0.0),
            commission_asset: Some(o.fee_asset),
            commission_amount: o.fee.to_f64().unwrap_or(0.0),
            trade_id: Some(o.trade_id),
            is_maker: o.is_maker,
            is_reduce_only: o.is_reduce_only,
            position_side: o.position_side,
            realized_pnl: o.realized_pnl.to_f64().unwrap_or(0.0),
            stp_mode: o.stp_mode,
            gtd: o.gtd.map(|v| v as i64),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingRatePo {
    pub id: i64,
    pub symbol: String,
    pub funding_rate: f64,
    pub funding_time: u64,
    pub mark_price: Option<f64>,
}

impl<'a> From<&duckdb::Row<'a>> for FundingRatePo {
    fn from(row: &duckdb::Row) -> Self {
        FundingRatePo {
            id: row.get(0).unwrap_or_default(),
            symbol: row.get(1).unwrap_or_default(),
            funding_rate: row.get(2).unwrap_or_default(),
            funding_time: row.get(3).unwrap_or_default(),
            mark_price: row.get(4).ok(), // 支持数据库字段为空
        }
    }
}

impl DuckDBPO for FundingRatePo {
    type Source = FundingRate;

    fn from_source(symbol: Option<&str>, source: &Self::Source) -> Self {
        let snow_flake = SnowyFlakeWrapper::new();
        let id = snow_flake.next_id_u64() as i64;
        FundingRatePo {
            id,
            symbol: symbol.expect("Symbol must be provided").to_string(),
            funding_rate: source.funding_rate,
            funding_time: source.funding_time,
            mark_price: source.mark_price,
        }
    }

    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>> {
        appender_params_from_iter(vec![
            &self.id as &dyn duckdb::ToSql,
            &self.symbol as &dyn duckdb::ToSql,
            &self.funding_rate as &dyn duckdb::ToSql,
            &self.funding_time as &dyn duckdb::ToSql,
            &self.mark_price as &dyn duckdb::ToSql,
        ])
    }
}
