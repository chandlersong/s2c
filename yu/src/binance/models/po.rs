use crate::duck_db::DuckDBPO;
use duckdb::appender_params_from_iter;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};
use std::string::ToString;
use yue::binance::bn_models::common::SymbolInfoTrait;
use yue::binance::bn_models::common::{PortfolioSpotOrderData, PortfolioSwapOrderData, SpotOrderData, SwapOrderData};
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_models::spot_websocket_stream::{KlineStreamPayload, SpotKlineData, TradeStreamPayload};
use yue::binance::bn_models::swap_restful::FundingRate;
use yue::binance::bn_models::swap_websocket_stream::SwapWebsocketKlineData;
use yue::models::InstrumentType;
use yue::tools::{SnowyFlakeWrapper, get_snow_flake_id_u64};

pub const SWAP_TEXT: &str = "PERPETUAL";

pub const TRADIFI_PERPETUAL_TEXT: &str = "TRADIFI_PERPETUAL";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinanceInstrument {
    pub id: i64,
    pub symbol: String,
    pub status: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub quote_asset_precision: i32,
    pub order_types: Vec<String>,
    pub symbol_type: InstrumentType,
    pub on_board_time: Option<u64>,
}

impl<T: SymbolInfoTrait> From<&T> for BinanceInstrument {
    fn from(symbol: &T) -> Self {
        Self {
            id: get_snow_flake_id_u64() as i64,
            symbol: symbol.symbol().to_string(),
            status: symbol.status().to_string(),
            base_asset: symbol.base_asset().to_string(),
            quote_asset: symbol.quote_asset().to_string(),
            quote_asset_precision: symbol.quote_precision(),
            order_types: symbol.order_types().clone(),
            symbol_type: InstrumentType::from_symbol_type(symbol.symbol_type()).expect("unsupported Binance symbol type"),
            on_board_time: symbol.get_on_board_time(),
        }
    }
}

impl BinanceInstrument {
    ///
    /// 数据库的场景。已经已经存在了。就是需要传入id。而不是自动生成
    ///
    pub fn from_symbol_info_with_id<T: SymbolInfoTrait>(symbol: &T, id: i64) -> Self {
        Self {
            id,
            symbol: symbol.symbol().to_string(),
            status: symbol.status().to_string(),
            base_asset: symbol.base_asset().to_string(),
            quote_asset: symbol.quote_asset().to_string(),
            quote_asset_precision: symbol.quote_precision(),
            order_types: symbol.order_types().clone(),
            symbol_type: InstrumentType::from_symbol_type(symbol.symbol_type()).expect("unsupported Binance symbol type"),
            on_board_time: symbol.get_on_board_time(),
        }
    }

    pub fn get_id_u64(&self) -> u64 {
        self.id as u64
    }

    pub fn into_proto_instrument(self) -> crate::sync::models::grpc_sync::Instrument {
        crate::sync::models::grpc_sync::Instrument {
            instrument_type: crate::sync::models::grpc_sync::InstrumentType::Binance as i32,
            payload: Some(crate::sync::models::grpc_sync::instrument::Payload::Binance(
                crate::sync::models::grpc_sync::BinanceInstrument::from(self),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotStreamTradeRecordPo {
    pub id: i64,
    pub event_time: i64,
    pub inst_id: u64,
    pub trade_id: i64,
    pub price: f64,
    pub qty: f64,
    pub trade_time: Option<i64>,
    pub is_buyer_maker: Option<bool>,
    pub created_at: i64,
}

impl DuckDBPO for SpotStreamTradeRecordPo {
    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>> {
        appender_params_from_iter(vec![
            &self.id as &dyn duckdb::ToSql,
            &self.event_time as &dyn duckdb::ToSql,
            &self.inst_id as &dyn duckdb::ToSql,
            &self.trade_id as &dyn duckdb::ToSql,
            &self.price as &dyn duckdb::ToSql,
            &self.qty as &dyn duckdb::ToSql,
            &self.trade_time as &dyn duckdb::ToSql,
            &self.is_buyer_maker as &dyn duckdb::ToSql,
            &self.created_at as &dyn duckdb::ToSql,
        ])
    }
}

impl SpotStreamTradeRecordPo {
    pub fn from_trade_stream_payload(payload: TradeStreamPayload, inst_id: u64) -> Self {
        SpotStreamTradeRecordPo {
            id: get_snow_flake_id_u64() as i64,
            event_time: payload.event_time as i64,
            inst_id,
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
    pub inst_id: u64,
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

impl KlinePo {
    pub fn to_spot_history_entry(&self) -> crate::sync::models::grpc_sync::HistoryEntry {
        self.to_history_entry_with_type(crate::sync::models::grpc_sync::HistoryType::BinanceSpot)
    }

    pub fn to_swap_history_entry(&self) -> crate::sync::models::grpc_sync::HistoryEntry {
        self.to_history_entry_with_type(crate::sync::models::grpc_sync::HistoryType::BinanceSwap)
    }

    fn to_history_entry_with_type(&self, history_type: crate::sync::models::grpc_sync::HistoryType) -> crate::sync::models::grpc_sync::HistoryEntry {
        use crate::sync::models::grpc_sync::{BinanceKline, InstrumentType, history_entry};
        crate::sync::models::grpc_sync::HistoryEntry {
            history_type: history_type.into(),
            instrument_type: InstrumentType::Binance.into(),
            inst_id: self.inst_id,
            payload: Some(history_entry::Payload::BinanceKline(BinanceKline::from(self.clone()))),
        }
    }
}

impl KlinePo {
    pub fn from_spot_kline_data(value: SpotKlineData, inst_id: u64) -> Self {
        KlinePo {
            id: get_snow_flake_id_u64() as i64,
            inst_id,
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

    pub fn from_kline_stream_payload(payload: KlineStreamPayload, inst_id: u64) -> Self {
        Self::from_spot_kline_data(payload.kline, inst_id)
    }

    pub fn from_binance_kline(source: BinanceKline, inst_id: u64) -> Self {
        KlinePo {
            id: get_snow_flake_id_u64() as i64,
            inst_id,
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

    pub fn from_swap_websocket_kline(value: SwapWebsocketKlineData, inst_id: u64) -> Self {
        KlinePo {
            id: get_snow_flake_id_u64() as i64,
            inst_id,
            candle_begin_time: value.start_time,
            open: value.open.to_f64().unwrap(),
            high: value.high.to_f64().unwrap(),
            low: value.low.to_f64().unwrap(),
            close: value.close.to_f64().unwrap(),
            volume: value.volume.to_f64().unwrap(),
            quote_volume: value.quote_asset_volume.to_f64().unwrap(),
            number_of_trades: value.number_of_trades,
            taker_buy_base_asset_volume: value.taker_buy_base_asset_volume.to_f64().unwrap(),
            taker_buy_quote_asset_volume: value.taker_buy_quote_asset_volume.to_f64().unwrap(),
            close_time: value.close.to_u64().unwrap(),
            interval: 0,
            first_trade_id: Some(-1),
            last_trade_id: Some(-1),
        }
    }
}

impl<'a> From<&duckdb::Row<'a>> for KlinePo {
    fn from(row: &duckdb::Row) -> Self {
        KlinePo {
            id: row.get(0).unwrap_or_default(),
            inst_id: row.get(1).unwrap_or_default(),
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
    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>> {
        appender_params_from_iter(vec![
            &self.id as &dyn duckdb::ToSql,
            &self.inst_id as &dyn duckdb::ToSql,
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
            "KlineData {{ id: {},interval {}, inst_id: {}, candle_begin_time: {}, open: {}, high: {}, low: {}, close: {}, volume: {}, quote_volume: {}, number_of_trades: {}, taker_buy_base_asset_volume: {}, taker_buy_quote_asset_volume: {}, close_time: {} }}",
            self.id,
            self.interval,
            self.inst_id,
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
    pub inst_id: u64,
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

impl SpotOrderPo {
    pub fn from_spot_order_data(data: SpotOrderData, inst_id: u64) -> Self {
        let payload = data.data;

        Self {
            event: payload.event,
            account_name: data.account_name,
            event_time: payload.event_time as i64,
            inst_id,
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

    pub fn from_portfolio_spot_order_data(data: PortfolioSpotOrderData, inst_id: u64) -> Self {
        let payload = data.data;
        Self {
            event: payload.event,
            account_name: data.account_name,
            event_time: payload.event_time as i64,
            inst_id,
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
    pub inst_id: u64,
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

impl SwapOrderPo {
    pub fn from_swap_order_data(data: SwapOrderData, inst_id: u64) -> Self {
        let payload = data.data;
        let o = payload.order.expect("Order data must be present");
        Self {
            event: payload.event,
            account_name: data.account_name,
            event_time: payload.event_time as i64,
            trade_time: payload.trade_time as i64,
            inst_id,
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

    pub fn from_portfolio_swap_order_data(data: PortfolioSwapOrderData, inst_id: u64) -> Self {
        let payload = data.data;
        let o = payload.order;
        Self {
            event: payload.event,
            account_name: data.account_name,
            event_time: payload.event_time as i64,
            trade_time: payload.trade_time as i64,
            inst_id,
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
    pub inst_id: u64,
    pub funding_rate: f64,
    pub funding_time: u64,
    pub mark_price: Option<f64>,
}

impl<'a> From<&duckdb::Row<'a>> for FundingRatePo {
    fn from(row: &duckdb::Row) -> Self {
        FundingRatePo {
            id: row.get(0).unwrap_or_default(),
            inst_id: row.get(1).unwrap_or_default(),
            funding_rate: row.get(2).unwrap_or_default(),
            funding_time: row.get(3).unwrap_or_default(),
            mark_price: row.get(4).ok(), // 支持数据库字段为空
        }
    }
}

impl FundingRatePo {
    pub fn to_swap_history_entry(&self) -> crate::sync::models::grpc_sync::HistoryEntry {
        use crate::sync::models::grpc_sync::{HistoryType, InstrumentType, history_entry};
        crate::sync::models::grpc_sync::HistoryEntry {
            history_type: HistoryType::BinanceSwapFundingRate.into(),
            instrument_type: InstrumentType::Binance.into(),
            inst_id: self.inst_id,
            payload: Some(history_entry::Payload::BinanceFundingRate(
                crate::sync::models::grpc_sync::BinanceFundingRate::from(self.clone()),
            )),
        }
    }

    pub fn from_funding_rate(source: FundingRate, inst_id: u64) -> Self {
        let snow_flake = SnowyFlakeWrapper::new();
        let id = snow_flake.next_id_u64() as i64;
        FundingRatePo {
            id,
            inst_id,
            funding_rate: source.funding_rate,
            funding_time: source.funding_time,
            mark_price: source.mark_price,
        }
    }
}

impl DuckDBPO for FundingRatePo {
    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>> {
        appender_params_from_iter(vec![
            &self.id as &dyn duckdb::ToSql,
            &self.inst_id as &dyn duckdb::ToSql,
            &self.funding_rate as &dyn duckdb::ToSql,
            &self.funding_time as &dyn duckdb::ToSql,
            &self.mark_price as &dyn duckdb::ToSql,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binance::bn_consts::BN_SYMBOL_STATUS_TRADING;
    use crate::sync::models::grpc_sync::{HistoryType, InstrumentType as SyncInstrumentType, history_entry};
    use yue::binance::bn_models::spot_restful::ExchangeSymbol;
    use yue::binance::bn_models::swap_restful::SwapExchangeSymbol;

    #[test]
    fn spot_exchange_symbol_converts_to_binance_instrument() {
        let symbol = ExchangeSymbol {
            symbol: "BTCUSDT".to_string(),
            status: BN_SYMBOL_STATUS_TRADING.to_string(),
            base_asset: "BTC".to_string(),
            base_asset_precision: 8,
            quote_asset: "USDT".to_string(),
            quote_precision: 8,
            quote_asset_precision: 8,
            base_commission_precision: 8,
            quote_commission_precision: 8,
            order_types: vec!["LIMIT".to_string()],
            iceberg_allowed: false,
            oco_allowed: false,
            quote_order_qty_market_allowed: true,
            allow_trailing_stop: false,
            cancel_replace_allowed: true,
            is_spot_trading_allowed: true,
            is_margin_trading_allowed: true,
            filters: Vec::new(),
            permissions: vec!["SPOT".to_string()],
            default_self_trade_prevention_mode: "NONE".to_string(),
            allowed_self_trade_prevention_modes: vec!["NONE".to_string()],
        };

        let instrument = BinanceInstrument::from(&symbol);

        assert_eq!(instrument.symbol, "BTCUSDT");
        assert!(matches!(instrument.symbol_type, InstrumentType::Spot));
        assert_eq!(instrument.quote_asset_precision, 8);
        assert_eq!(instrument.on_board_time, None);
    }

    #[test]
    fn swap_exchange_symbol_converts_to_binance_instrument() {
        let symbol = SwapExchangeSymbol {
            symbol: "BTCUSDT".to_string(),
            pair: "BTCUSDT".to_string(),
            contract_type: "PERPETUAL".to_string(),
            delivery_date: Some(0),
            onboard_date: Some(1_600_000_000_000),
            status: BN_SYMBOL_STATUS_TRADING.to_string(),
            maint_margin_percent: None,
            required_margin_percent: None,
            base_asset: "BTC".to_string(),
            quote_asset: "USDT".to_string(),
            margin_asset: "USDT".to_string(),
            price_precision: 2,
            quantity_precision: 3,
            base_asset_precision: 8,
            quote_precision: 8,
            underlying_type: None,
            underlying_sub_type: None,
            settle_plan: None,
            trigger_protect: None,
            filters: Vec::new(),
            order_types: vec!["LIMIT".to_string()],
            time_in_force: None,
            liquidation_fee: None,
            market_take_bound: None,
            max_move_order_limit: None,
            price_scale: None,
        };

        let instrument = BinanceInstrument::from(&symbol);

        assert_eq!(instrument.symbol, "BTCUSDT");
        assert!(matches!(instrument.symbol_type, InstrumentType::Swap));
        assert_eq!(instrument.on_board_time, Some(1_600_000_000_000));
    }

    // 测试目的：验证 Spot Kline stream payload 转换时写入传入的 instrument id。
    // 准备数据/过程：构造带 BTCUSDT Kline 的 stream payload，并传入 instrument id 42。
    // 预期结果：PO 保存 id 42，且时间、周期和成交 ID 字段保持正确。
    #[test]
    fn kline_stream_payload_converts_to_kline_po() {
        let payload = KlineStreamPayload {
            event: "kline".to_string(),
            event_time: 1_700_000_000_000,
            symbol: "BTCUSDT".to_string(),
            kline: SpotKlineData {
                start_time: 1_700_000_000_000,
                close_time: 1_700_000_299_999,
                symbol: "BTCUSDT".to_string(),
                interval: "5m".to_string(),
                first_trade_id: 10,
                last_trade_id: 20,
                open: 100.into(),
                close: 105.into(),
                high: 110.into(),
                low: 95.into(),
                volume: 12.into(),
                trade_count: 11,
                is_closed: true,
                quote_volume: 1_200.into(),
                taker_buy_base_volume: 7.into(),
                taker_buy_quote_volume: 700.into(),
                ignore: String::new(),
            },
        };

        let po = KlinePo::from_kline_stream_payload(payload, 42);

        assert_eq!(po.inst_id, 42);
        assert_eq!(po.candle_begin_time, 1_700_000_000_000);
        assert_eq!(po.close_time, 1_700_000_299_999);
        assert_eq!(po.interval, INTERVAL_5M);
        assert_eq!(po.first_trade_id, Some(10));
        assert_eq!(po.last_trade_id, Some(20));
    }

    // 测试目的：验证 Spot trade stream payload 转换时写入传入的 instrument id。
    // 准备数据/过程：构造完整 trade payload，并传入 instrument id 42。
    // 预期结果：PO 保存 id 42，且成交事件字段保持正确。
    #[test]
    fn trade_stream_payload_preserves_event_fields() {
        let payload = TradeStreamPayload {
            event: "trade".to_string(),
            event_time: 1_700_000_000_000,
            symbol: "BTCUSDT".to_string(),
            trade_id: 10,
            price: 100.into(),
            qty: 2.into(),
            trade_time: 1_700_000_000_001,
            is_buyer_maker: true,
            ignore: false,
        };

        let po = SpotStreamTradeRecordPo::from_trade_stream_payload(payload, 42);

        assert_eq!(po.inst_id, 42);
        assert_eq!(po.event_time, 1_700_000_000_000);
        assert_eq!(po.trade_time, Some(1_700_000_000_001));
        assert_eq!(po.is_buyer_maker, Some(true));
    }

    // 测试目的：验证 Binance Swap Kline PO 转换为同步历史条目时使用 Swap 历史类型。
    // 准备数据/过程：构造包含毫秒时间戳、instrument ID 和可选成交 ID 的 KlinePo，并调用 Swap 转换方法。
    // 预期结果：协议条目保留 Binance instrument 类型、Swap history 类型、instrument ID 及 Kline 字段。
    #[test]
    fn swap_kline_po_converts_to_binance_swap_history_entry() {
        let kline = KlinePo {
            id: 3,
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
            first_trade_id: Some(-1),
            last_trade_id: Some(-1),
        };

        let entry = kline.to_swap_history_entry();

        assert_eq!(HistoryType::try_from(entry.history_type).unwrap(), HistoryType::BinanceSwap);
        assert_eq!(SyncInstrumentType::try_from(entry.instrument_type).unwrap(), SyncInstrumentType::Binance);
        assert_eq!(entry.inst_id, 42);
        let Some(history_entry::Payload::BinanceKline(payload)) = entry.payload else {
            panic!("expected Binance Kline payload");
        };
        assert_eq!(payload.id, 3);
        assert_eq!(payload.candle_begin_time, kline.candle_begin_time);
        assert_eq!(payload.close_time, kline.close_time);
        assert_eq!(payload.first_trade_id, Some(-1));
    }
}
