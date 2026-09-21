use crate::duck_db::DuckDBPO;
use crate::errors::YuError;
use bon::Builder;
use duckdb::{Row, Rows, appender_params_from_iter};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use yue::okx::models::restful::{CandleResponse, InstrumentInfo, OptionSummaryDetail};
use yue::okx::models::websocket::KlinePayload;
use yue::tools::get_snow_flake_id_u64;

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
pub struct OkxKlinePo {
    pub id: u64,
    pub inst_id: u64,
    pub ts: u64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub vol: f64,
    pub vol_ccy: f64,
    pub vol_ccy_quote: f64,
    pub confirm: u8,
}

impl OkxKlinePo {
    pub fn from_kline_response(inst_id: u64, response: CandleResponse) -> Vec<Self> {
        let mut res = Vec::<Self>::new();
        for candle in response.data {
            res.push(Self {
                id: get_snow_flake_id_u64(),
                inst_id,
                ts: candle[0].parse().unwrap(),
                open: candle[1].parse().unwrap(),
                high: candle[2].parse().unwrap(),
                low: candle[3].parse().unwrap(),
                close: candle[4].parse().unwrap(),
                vol: candle[5].parse().unwrap(),
                vol_ccy: candle[6].parse().unwrap(),
                vol_ccy_quote: candle[7].parse().unwrap(),
                confirm: candle[8].parse().unwrap(),
            });
        }
        res
    }

    pub fn from_ws_response(inst_id: u64, payload: &KlinePayload) -> Vec<Self> {
        let mut res = Vec::<Self>::new();
        for candle in &payload.data {
            res.push(Self {
                id: get_snow_flake_id_u64(),
                inst_id,
                ts: candle[0].parse().unwrap(),
                open: candle[1].parse().unwrap(),
                high: candle[2].parse().unwrap(),
                low: candle[3].parse().unwrap(),
                close: candle[4].parse().unwrap(),
                vol: candle[5].parse().unwrap(),
                vol_ccy: candle[6].parse().unwrap(),
                vol_ccy_quote: candle[7].parse().unwrap(),
                confirm: candle[8].parse().unwrap(),
            });
        }
        res
    }
}

impl DuckDBPO for OkxKlinePo {
    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>> {
        appender_params_from_iter(vec![
            &self.id as &dyn duckdb::ToSql,
            &self.inst_id as &dyn duckdb::ToSql,
            &self.ts as &dyn duckdb::ToSql,
            &self.open as &dyn duckdb::ToSql,
            &self.high as &dyn duckdb::ToSql,
            &self.low as &dyn duckdb::ToSql,
            &self.close as &dyn duckdb::ToSql,
            &self.vol as &dyn duckdb::ToSql,
            &self.vol_ccy as &dyn duckdb::ToSql,
            &self.vol_ccy_quote as &dyn duckdb::ToSql,
            &self.confirm as &dyn duckdb::ToSql,
        ])
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
pub struct OkxOptionSummaryPo {
    pub id: u64,
    pub inst_id: u64,
    pub inst_identify: String,
    pub inst_type: String,
    pub uly: Option<String>,
    // 获取的时间
    pub acquire_ts: u64,
    //从元数据读取的时间
    pub server_ts: u64,
    pub ask_vol: Option<f64>,
    pub bid_vol: Option<f64>,
    pub delta: Option<f64>,
    pub delta_bs: Option<f64>,
    pub fwd_px: Option<f64>,
    pub gamma: Option<f64>,
    pub gamma_bs: Option<f64>,
    pub lever: Option<f64>,
    pub mark_vol: Option<f64>,
    pub real_vol: Option<f64>,
    pub vol_lv: Option<f64>,
    pub theta: Option<f64>,
    pub theta_bs: Option<f64>,
    pub vega: Option<f64>,
    pub vega_bs: Option<f64>,
}
impl OkxOptionSummaryPo {
    pub fn from_detail(inst_id: u64, acquire_ts: u64, detail: &OptionSummaryDetail) -> Self {
        Self {
            id: get_snow_flake_id_u64(),
            inst_id,
            inst_identify: detail.inst_id.clone(),
            inst_type: detail.inst_type.clone(),
            uly: detail.uly.clone(),
            acquire_ts,
            server_ts: detail.ts.unwrap_or_default(),
            ask_vol: detail.ask_vol.and_then(|d| d.to_f64()),
            bid_vol: detail.bid_vol.and_then(|d| d.to_f64()),
            delta: detail.delta.and_then(|d| d.to_f64()),
            delta_bs: detail.delta_bs.and_then(|d| d.to_f64()),
            fwd_px: detail.fwd_px.and_then(|d| d.to_f64()),
            gamma: detail.gamma.and_then(|d| d.to_f64()),
            gamma_bs: detail.gamma_bs.and_then(|d| d.to_f64()),
            lever: detail.lever.and_then(|d| d.to_f64()),
            mark_vol: detail.mark_vol.and_then(|d| d.to_f64()),
            real_vol: detail.real_vol.and_then(|d| d.to_f64()),
            vol_lv: detail.vol_lv.and_then(|d| d.to_f64()),
            theta: detail.theta.and_then(|d| d.to_f64()),
            theta_bs: detail.theta_bs.and_then(|d| d.to_f64()),
            vega: detail.vega.and_then(|d| d.to_f64()),
            vega_bs: detail.vega_bs.and_then(|d| d.to_f64()),
        }
    }
}

impl DuckDBPO for OkxOptionSummaryPo {
    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>> {
        appender_params_from_iter(vec![
            &self.id as &dyn duckdb::ToSql,
            &self.inst_id as &dyn duckdb::ToSql,
            &self.inst_identify as &dyn duckdb::ToSql,
            &self.inst_type as &dyn duckdb::ToSql,
            &self.uly as &dyn duckdb::ToSql,
            &self.acquire_ts as &dyn duckdb::ToSql,
            &self.server_ts as &dyn duckdb::ToSql,
            &self.ask_vol as &dyn duckdb::ToSql,
            &self.bid_vol as &dyn duckdb::ToSql,
            &self.delta as &dyn duckdb::ToSql,
            &self.delta_bs as &dyn duckdb::ToSql,
            &self.fwd_px as &dyn duckdb::ToSql,
            &self.gamma as &dyn duckdb::ToSql,
            &self.gamma_bs as &dyn duckdb::ToSql,
            &self.lever as &dyn duckdb::ToSql,
            &self.mark_vol as &dyn duckdb::ToSql,
            &self.real_vol as &dyn duckdb::ToSql,
            &self.vol_lv as &dyn duckdb::ToSql,
            &self.theta as &dyn duckdb::ToSql,
            &self.theta_bs as &dyn duckdb::ToSql,
            &self.vega as &dyn duckdb::ToSql,
            &self.vega_bs as &dyn duckdb::ToSql,
        ])
    }
}

impl<'a> TryFrom<&'a Row<'a>> for OkxOptionSummaryPo {
    type Error = YuError;

    fn try_from(row: &Row) -> Result<Self, Self::Error> {
        let id: u64 = row.get(0)?;
        let inst_id: u64 = row.get(1)?;
        let inst_identify: String = row.get(2)?;
        let inst_type: String = row.get(3)?;
        let uly: Option<String> = row.get(4)?;
        let acquire_ts: u64 = row.get(5)?;
        let server_ts: u64 = row.get(6)?;
        let ask_vol: Option<f64> = row.get(7)?;
        let bid_vol: Option<f64> = row.get(8)?;
        let delta: Option<f64> = row.get(9)?;
        let delta_bs: Option<f64> = row.get(10)?;
        let fwd_px: Option<f64> = row.get(11)?;
        let gamma: Option<f64> = row.get(12)?;
        let gamma_bs: Option<f64> = row.get(13)?;
        let lever: Option<f64> = row.get(14)?;
        let mark_vol: Option<f64> = row.get(15)?;
        let real_vol: Option<f64> = row.get(16)?;
        let vol_lv: Option<f64> = row.get(17)?;
        let theta: Option<f64> = row.get(18)?;
        let theta_bs: Option<f64> = row.get(19)?;
        let vega: Option<f64> = row.get(20)?;
        let vega_bs: Option<f64> = row.get(21)?;

        Ok(OkxOptionSummaryPo {
            id,
            inst_id,
            inst_identify,
            inst_type,
            uly,
            acquire_ts,
            server_ts,
            ask_vol,
            bid_vol,
            delta,
            delta_bs,
            fwd_px,
            gamma,
            gamma_bs,
            lever,
            mark_vol,
            real_vol,
            vol_lv,
            theta,
            theta_bs,
            vega,
            vega_bs,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
pub struct InstrumentPo {
    pub id: u64,
    pub inst_identify: String,
    pub inst_type: String,
    pub inst_family: Option<String>,
    pub base_ccy: String,
    pub quote_ccy: Option<String>,
    pub settle_ccy: Option<String>,
    pub list_time: Option<u64>,
    pub exp_time: Option<u64>,
    pub tick_sz: Option<f64>,
    pub lot_sz: Option<f64>,
    pub min_sz: Option<f64>,
    pub alias: Option<String>,
    pub state: Option<String>,
    pub inst_id_code: Option<String>,
    pub inst_category: Option<String>,
}

impl InstrumentPo {
    pub fn from_db_to_vec(mut rows: Rows) -> Result<Vec<InstrumentPo>, YuError> {
        let mut res = Vec::<InstrumentPo>::new();
        while let Some(row) = rows.next()? {
            let po = Self::try_from(row)?;
            res.push(po);
        }
        Ok(res)
    }
}

impl DuckDBPO for InstrumentPo {
    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>> {
        appender_params_from_iter(vec![
            &self.id as &dyn duckdb::ToSql,
            &self.inst_identify as &dyn duckdb::ToSql,
            &self.inst_type as &dyn duckdb::ToSql,
            &self.inst_family as &dyn duckdb::ToSql,
            &self.base_ccy as &dyn duckdb::ToSql,
            &self.quote_ccy as &dyn duckdb::ToSql,
            &self.settle_ccy as &dyn duckdb::ToSql,
            &self.list_time as &dyn duckdb::ToSql,
            &self.exp_time as &dyn duckdb::ToSql,
            &self.tick_sz as &dyn duckdb::ToSql,
            &self.lot_sz as &dyn duckdb::ToSql,
            &self.min_sz as &dyn duckdb::ToSql,
            &self.alias as &dyn duckdb::ToSql,
            &self.state as &dyn duckdb::ToSql,
            &self.inst_id_code as &dyn duckdb::ToSql,
            &self.inst_category as &dyn duckdb::ToSql,
        ])
    }
}

impl From<InstrumentInfo> for InstrumentPo {
    fn from(info: InstrumentInfo) -> Self {
        InstrumentPo {
            id: get_snow_flake_id_u64(),
            inst_identify: info.inst_id,
            inst_type: info.inst_type,
            inst_family: info.inst_family,
            base_ccy: info.base_ccy,
            quote_ccy: info.quote_ccy,
            settle_ccy: info.settle_ccy,
            list_time: info.list_time,
            exp_time: info.exp_time,
            tick_sz: info.tick_sz.and_then(|d| d.to_f64()),
            lot_sz: info.lot_sz.and_then(|d| d.to_f64()),
            min_sz: info.min_sz.and_then(|d| d.to_f64()),
            alias: info.alias,
            state: info.state,
            inst_id_code: info.inst_id_code.map(|i| i.to_string()),
            inst_category: info.inst_category,
        }
    }
}

impl<'a> TryFrom<&'a Row<'a>> for InstrumentPo {
    type Error = YuError;

    fn try_from(row: &Row) -> Result<Self, Self::Error> {
        let id: u64 = row.get(0)?;
        let inst_identify: String = row.get(1)?;
        let inst_type: String = row.get(2)?;
        let inst_family: Option<String> = row.get(3)?;
        let base_ccy: String = row.get(4)?;
        let quote_ccy: Option<String> = row.get(5)?;
        let settle_ccy: Option<String> = row.get(6)?;
        let list_time: Option<u64> = row.get(7)?;
        let exp_time: Option<u64> = row.get(8)?;
        let tick_sz: Option<f64> = row.get(9)?;
        let lot_sz: Option<f64> = row.get(10)?;
        let min_sz: Option<f64> = row.get(11)?;
        let alias: Option<String> = row.get(12)?;
        let state: Option<String> = row.get(13)?;
        let inst_id_code: Option<String> = row.get(14)?;
        let inst_category: Option<String> = row.get(15)?;

        Ok(InstrumentPo {
            id,
            inst_identify,
            inst_type,
            inst_family,
            base_ccy,
            quote_ccy,
            settle_ccy,
            list_time,
            exp_time,
            tick_sz,
            lot_sz,
            min_sz,
            alias,
            state,
            inst_id_code,
            inst_category,
        })
    }
}
