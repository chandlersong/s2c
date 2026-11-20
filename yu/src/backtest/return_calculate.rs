//!
//! 在本mod中。想要作为一个回测结果的trait。
//! 在构思的时候，我想做成Sequence为主的模式。
//!
//! 主要原因是这个算是比较核心的一个模块。最好代码共享。而batch，本质是Sequence的一个组合。
//! 除非用polar这类作用，Batch也没有任何优化
//!

use crate::backtest::core::ReturnCalculateTrait;
use crate::errors::YuError;
use crate::models::BacktestInst;
use li::tools::time::UnixTimeStamp;
use polars::prelude::{DataFrame, IntoLazy, JoinArgs, JoinType, SortMultipleOptions, col, lit};
use rust_decimal::prelude::FromPrimitive;
use std::collections::{HashMap, HashSet};
use yue::models::{Decimal, HistoryInterval};

#[derive(Clone)]
pub enum OrderReason {
    ReBalance,
    PositionChange,
}

#[derive(Clone)]
pub struct PNL {
    pub realized_pnl: Decimal,
    pub unrealized_pnl: Decimal,
    pub commission: Decimal,
    pub turnover: Decimal,
    pub hold_value: Decimal,
}

#[derive(Clone)]
pub struct OrderInfo {
    inst_id: u64,
    pos: Decimal,
    order_amount: Decimal,
    unrealized_pnl: Decimal,
    realized_pnl: Decimal,
    entry_price: Decimal,
    order_reason: OrderReason,
}
#[derive(Clone)]
pub struct AccountSnapshot {
    timestamp: UnixTimeStamp,
    wallet_balance: Decimal,
    margin_balance: Decimal,
    realized_pnl: Decimal,
    unrealized_pnl: Decimal,
    commission: Decimal,
    margin_ratio: Decimal,
    turnover_monthly: Decimal,
    leverage: Decimal,
}

#[derive(Clone)]
pub struct BacktestResult {
    pub account_history: Vec<AccountSnapshot>,
    pub order_infos: Vec<OrderInfo>,
}

#[derive(Clone)]
pub struct TickBacktestResult {
    pub account_snapshot: AccountSnapshot,
    pub order_infos: Vec<OrderInfo>,
}

#[derive(Clone)]
pub struct BacktestPosition {
    position: Decimal,
    entry_price: Decimal,
}

#[derive(Debug, PartialEq)]
pub struct PositionTarget {
    inst_id: u64,
    position_ratio: Decimal,
    close: Decimal,
    trade_price: Option<Decimal>,
    next_close: Option<Decimal>,
}

#[derive(Debug, PartialEq)]
pub struct TimestampPositionTarget {
    timestamp: i64,
    value: Vec<PositionTarget>,
}
#[derive(Clone)]
pub struct OneTickPNL {
    pub pnl: PNL,
    pub order_info: OrderInfo,
}

fn decimal_from_f64(value: f64, field: &str, row: usize) -> Result<Decimal, YuError> {
    Decimal::from_f64(value).ok_or_else(|| YuError::DataAlignmentError(format!("{field} cannot be represented as Decimal at row {row}")))
}

fn match_position_market_data<F>(
    position_data: &DataFrame,
    market_data: &DataFrame,
    tick_interval: &HistoryInterval,
    mut process_timestamp: F,
) -> Result<(), YuError>
where
    F: FnMut(TimestampPositionTarget) -> Result<(), YuError>,
{
    let position_timestamp_column = if position_data.get_column_index("time_stamp").is_some() {
        "time_stamp"
    } else {
        "timestamp"
    };
    let position_timestamps = position_data
        .column(position_timestamp_column)
        .and_then(|column| column.i64())
        .map_err(|error| YuError::DataAlignmentError(error.to_string()))?;
    let position_inst_ids = position_data
        .column("inst_id")
        .and_then(|column| column.i64())
        .map_err(|error| YuError::DataAlignmentError(error.to_string()))?;
    let mut position_keys = HashSet::with_capacity(position_data.height());
    for row in 0..position_data.height() {
        let timestamp = position_timestamps
            .get(row)
            .ok_or_else(|| YuError::DataAlignmentError(format!("null {position_timestamp_column} at row {row}")))?;
        let inst_id = position_inst_ids
            .get(row)
            .ok_or_else(|| YuError::DataAlignmentError(format!("null position inst_id at row {row}")))?;
        if !position_keys.insert((timestamp, inst_id)) {
            return Err(YuError::DataAlignmentError(format!(
                "duplicate position key for timestamp {timestamp} and inst_id {inst_id}"
            )));
        }
    }

    let interval_ms =
        i64::try_from(tick_interval.to_milliseconds()).map_err(|_| YuError::DataAlignmentError("tick interval is too large".to_string()))?;
    let positions = position_data.clone().lazy().with_columns([
        col(position_timestamp_column).alias("timestamp"),
        (col(position_timestamp_column) + lit(interval_ms)).alias("next_timestamp"),
    ]);
    let current_market = market_data
        .clone()
        .lazy()
        .select([col("window_end").alias("timestamp"), col("inst_id"), col("close")]);
    let next_market = market_data.clone().lazy().select([
        col("window_end").alias("next_timestamp"),
        col("inst_id"),
        col("open").alias("trade_price"),
        col("close").alias("next_close"),
    ]);
    let joined = positions
        .join(
            current_market,
            [col("timestamp"), col("inst_id")],
            [col("timestamp"), col("inst_id")],
            JoinArgs::new(JoinType::Left),
        )
        .join(
            next_market,
            [col("next_timestamp"), col("inst_id")],
            [col("next_timestamp"), col("inst_id")],
            JoinArgs::new(JoinType::Left),
        )
        .select([
            col("timestamp"),
            col("inst_id"),
            col("position_ratio"),
            col("close"),
            col("trade_price"),
            col("next_close"),
        ])
        .collect()
        .map_err(|error| YuError::DataAlignmentError(error.to_string()))?
        .sort(["timestamp", "inst_id"], SortMultipleOptions::default())
        .map_err(|error| YuError::DataAlignmentError(error.to_string()))?;

    if joined.height() > position_data.height() {
        return Err(YuError::DataAlignmentError("duplicate market data matched to position rows".to_string()));
    }

    let timestamps = joined
        .column("timestamp")
        .and_then(|column| column.i64())
        .map_err(|error| YuError::DataAlignmentError(error.to_string()))?;
    let inst_ids = joined
        .column("inst_id")
        .and_then(|column| column.i64())
        .map_err(|error| YuError::DataAlignmentError(error.to_string()))?;
    let position_ratios = joined
        .column("position_ratio")
        .and_then(|column| column.f64())
        .map_err(|error| YuError::DataAlignmentError(error.to_string()))?;
    let closes = joined
        .column("close")
        .and_then(|column| column.f64())
        .map_err(|error| YuError::DataAlignmentError(error.to_string()))?;
    let trade_prices = joined
        .column("trade_price")
        .and_then(|column| column.f64())
        .map_err(|error| YuError::DataAlignmentError(error.to_string()))?;
    let next_closes = joined
        .column("next_close")
        .and_then(|column| column.f64())
        .map_err(|error| YuError::DataAlignmentError(error.to_string()))?;
    let mut current_timestamp = None;
    let mut current_positions = Vec::new();

    for row in 0..joined.height() {
        let timestamp = timestamps
            .get(row)
            .ok_or_else(|| YuError::DataAlignmentError(format!("null {position_timestamp_column} at row {row}")))?;
        let inst_id = inst_ids
            .get(row)
            .ok_or_else(|| YuError::DataAlignmentError(format!("null position inst_id at row {row}")))?;
        let inst_id = u64::try_from(inst_id).map_err(|_| YuError::DataAlignmentError(format!("negative position inst_id at row {row}")))?;
        let position_ratio = position_ratios
            .get(row)
            .ok_or_else(|| YuError::DataAlignmentError(format!("null position_ratio at row {row}")))?;
        if !position_ratio.is_finite() {
            return Err(YuError::DataAlignmentError(format!("position_ratio must be finite at row {row}")));
        }
        let position_ratio = decimal_from_f64(position_ratio, "position_ratio", row)?;
        let close = closes
            .get(row)
            .ok_or_else(|| YuError::DataAlignmentError(format!("market data not found for inst_id {inst_id} at {timestamp}")))?;
        if !close.is_finite() || close <= 0.0 {
            return Err(YuError::DataAlignmentError(format!(
                "market close must be finite and positive at row {row}"
            )));
        }
        let close = decimal_from_f64(close, "close", row)?;
        let trade_price = trade_prices.get(row);
        if trade_price.is_some_and(|price| !price.is_finite() || price <= 0.0) {
            return Err(YuError::DataAlignmentError(format!(
                "next market open must be finite and positive at row {row}"
            )));
        }
        let trade_price = trade_price.map(|price| decimal_from_f64(price, "trade_price", row)).transpose()?;
        let next_close = next_closes.get(row);
        if next_close.is_some_and(|price| !price.is_finite() || price <= 0.0) {
            return Err(YuError::DataAlignmentError(format!(
                "next market close must be finite and positive at row {row}"
            )));
        }
        let next_close = next_close.map(|price| decimal_from_f64(price, "next_close", row)).transpose()?;

        if let Some(previous_timestamp) = current_timestamp {
            if previous_timestamp != timestamp {
                process_timestamp(TimestampPositionTarget {
                    timestamp: previous_timestamp,
                    value: std::mem::take(&mut current_positions),
                })?;
            }
        }
        current_timestamp = Some(timestamp);
        current_positions.push(PositionTarget {
            inst_id,
            position_ratio,
            close,
            trade_price,
            next_close,
        });
    }

    if let Some(timestamp) = current_timestamp {
        process_timestamp(TimestampPositionTarget {
            timestamp,
            value: current_positions,
        })?;
    }

    Ok(())
}

#[derive(Clone)]
pub struct SequencePosition<'p> {
    position_state: &'p HashMap<u64, BacktestPosition>,
    instruments: &'p HashMap<u64, BacktestInst>,
}

fn one_instrument_one_tick(target: &PositionTarget, position: &BacktestPosition, instruments: &BacktestInst) -> Result<OneTickPNL, YuError> {
    todo!()
}

pub struct SequenceCalculateReturn {
    tick_interval: HistoryInterval,
    account: AccountSnapshot,
}

impl SequenceCalculateReturn {
    pub fn new(tick_interval: HistoryInterval, initial_account: AccountSnapshot) -> Self {
        SequenceCalculateReturn {
            tick_interval,
            account: initial_account,
        }
    }
}

impl ReturnCalculateTrait for SequenceCalculateReturn {
    type Position<'p> = SequencePosition<'p>;
    type Market<'p> = &'p DataFrame;
    type Result = TickBacktestResult;

    fn calculate<'p>(&self, position_data: Self::Position<'p>, market_data: Self::Market<'p>) -> Result<Self::Result, YuError> {
        todo!()
    }
}

pub struct BatchCalculateReturn {
    position_state: HashMap<u64, BacktestPosition>, // inst_id -> position
    tick_interval: HistoryInterval,
    instruments: HashMap<u64, BacktestInst>, // inst_id -> instrument
}

impl BatchCalculateReturn {}

impl ReturnCalculateTrait for BatchCalculateReturn {
    type Position<'p> = DataFrame;
    type Market<'p> = DataFrame;
    type Result = BacktestResult;

    /// # 参数
    /// ## position_data:
    /// DataFrame, 包含了每个时间点的仓位信息，包含了timestamp, inst_id, position_ratio, direction等字段
    /// 1. timestamp应该是连续的。也就是说在max和min之间的每个时间点都应该有数据，因为同一时间，inst_id个数不一定，所以不能确定。
    /// 2. position_ratio是仓位占总资金的比例，可能为负，direction是方向，1表示多头，-1表示空头
    /// ## market_data: DataFrame, 包含了每个时间点的市场信息，inst_id, price等字段
    /// 标准的K线的数据。必须有window_end和close两个字段,时间戳是以window_end为准
    ///
    /// # 需求
    /// 简单的来说，
    /// 1. 账户变化。
    ///     - 余额变化
    ///     - 收益变化
    ///     - 换手率
    /// 2. 有多少笔交易。
    ///
    /// # 设计
    ///
    /// ## struct维护
    /// 1. 账户信息。记录需要变化的内容
    /// 2. instrument的基础信息，比如最小交易单位，最小变动价格等
    ///
    /// ## 方法
    /// 1. 分组position_data的tick，然后在market_data中找到对应的价格，计算账户变化和交易信息
    /// 2. 每个instrument+tick单独计算，然后汇总。
    ///
    ///
    fn calculate<'p>(&self, position_data: Self::Position<'p>, market_data: Self::Market<'p>) -> Result<Self::Result, YuError> {
        match_position_market_data(&position_data, &market_data, &self.tick_interval, |_| Ok(()))?;
        Ok(BacktestResult {
            account_history: vec![],
            order_infos: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use polars::prelude::{NamedFrom, Series};

    fn decimal(value: &str) -> Decimal {
        value.parse().unwrap()
    }

    fn make_positions(timestamp_column: &str, timestamps: Vec<i64>, inst_ids: Vec<i64>, position_ratios: Vec<f64>) -> DataFrame {
        DataFrame::new(vec![
            Series::new(timestamp_column.into(), timestamps).into(),
            Series::new("inst_id".into(), inst_ids).into(),
            Series::new("position_ratio".into(), position_ratios).into(),
        ])
        .unwrap()
    }

    fn make_market(window_ends: Vec<i64>, inst_ids: Vec<i64>, closes: Vec<f64>) -> DataFrame {
        make_market_with_opens(window_ends, inst_ids, closes.clone(), closes)
    }

    fn make_market_with_opens(window_ends: Vec<i64>, inst_ids: Vec<i64>, opens: Vec<f64>, closes: Vec<f64>) -> DataFrame {
        DataFrame::new(vec![
            Series::new("window_end".into(), window_ends).into(),
            Series::new("inst_id".into(), inst_ids).into(),
            Series::new("open".into(), opens).into(),
            Series::new("close".into(), closes).into(),
        ])
        .unwrap()
    }

    // 测试目的：验证结果按时间戳和 instrument 升序分组，且持仓键唯一时能匹配行情。
    // 准备数据/过程：构造乱序时间戳和行情数据，并在一个时间戳下放入两个不同 instrument。
    // 预期结果：时间戳升序返回，同一时间戳下的 instrument 按序，trade_price 匹配下一周期 open，next_close 匹配下一周期 close。
    #[test]
    fn groups_unique_position_keys_by_sorted_timestamp_and_instrument() {
        let positions = make_positions("time_stamp", vec![120_000_i64, 60_000, 60_000], vec![2_i64, 3, 1], vec![0.2, 0.3, 0.1]);
        let market = make_market_with_opens(
            vec![120_000_i64, 120_000, 60_000, 60_000],
            vec![2_i64, 1, 1, 3],
            vec![21.0_f64, 10.5, 10.0, 12.5],
            vec![22.0_f64, 12.0, 11.0, 13.0],
        );

        let mut matched = Vec::new();
        match_position_market_data(&positions, &market, &HistoryInterval::OneMinute, |group| {
            matched.push(group);
            Ok(())
        })
        .unwrap();

        assert_eq!(
            matched,
            vec![
                TimestampPositionTarget {
                    timestamp: 60_000,
                    value: vec![
                        PositionTarget {
                            inst_id: 1,
                            position_ratio: decimal("0.1"),
                            close: decimal("11"),
                            trade_price: Some(decimal("10.5")),
                            next_close: Some(decimal("12")),
                        },
                        PositionTarget {
                            inst_id: 3,
                            position_ratio: decimal("0.3"),
                            close: decimal("13"),
                            trade_price: None,
                            next_close: None,
                        },
                    ],
                },
                TimestampPositionTarget {
                    timestamp: 120_000,
                    value: vec![PositionTarget {
                        inst_id: 2,
                        position_ratio: decimal("0.2"),
                        close: decimal("22"),
                        trade_price: None,
                        next_close: None,
                    }],
                },
            ]
        );
    }

    // 测试目的：验证持仓数据中相同 timestamp 和 inst_id 不允许重复。
    // 准备数据/过程：构造两条键相同的持仓行和对应的一条行情，调用匹配函数。
    // 预期结果：在行情匹配前返回包含重复 timestamp 和 inst_id 的错误。
    #[test]
    fn rejects_duplicate_position_timestamp_and_instrument_keys() {
        let positions = make_positions("time_stamp", vec![100_i64, 100], vec![1_i64, 1], vec![0.2, 0.3]);
        let market = make_market(vec![100_i64], vec![1_i64], vec![11.0_f64]);

        let error = match_position_market_data(&positions, &market, &HistoryInterval::OneMinute, |_| Ok(())).unwrap_err();

        assert!(matches!(&error, YuError::DataAlignmentError(_)));
        assert!(error.to_string().contains("duplicate position key for timestamp 100 and inst_id 1"));
    }

    // 测试目的：验证标的下架后没有下一周期行情时，当前持仓仍保留且不会阻断后续时间点。
    // 准备数据/过程：为标的 1 只提供当前行情，并提供标的 2 的后续行情后执行匹配。
    // 预期结果：标的 1 的 trade_price 和 next_close 均为 None，标的 2 匹配下一周期 open 和 close。
    #[test]
    fn keeps_delisted_position_without_next_close() {
        let positions = make_positions("time_stamp", vec![60_000_i64, 120_000], vec![1_i64, 2], vec![0.4, 0.6]);
        let market = make_market(vec![60_000_i64, 120_000, 180_000], vec![1_i64, 2, 2], vec![11.0_f64, 22.0, 23.0]);

        let mut matched = Vec::new();
        match_position_market_data(&positions, &market, &HistoryInterval::OneMinute, |group| {
            matched.push(group);
            Ok(())
        })
        .unwrap();

        assert_eq!(
            matched,
            vec![
                TimestampPositionTarget {
                    timestamp: 60_000,
                    value: vec![PositionTarget {
                        inst_id: 1,
                        position_ratio: decimal("0.4"),
                        close: decimal("11"),
                        trade_price: None,
                        next_close: None,
                    }],
                },
                TimestampPositionTarget {
                    timestamp: 120_000,
                    value: vec![PositionTarget {
                        inst_id: 2,
                        position_ratio: decimal("0.6"),
                        close: decimal("22"),
                        trade_price: Some(decimal("23")),
                        next_close: Some(decimal("23")),
                    }],
                },
            ]
        );
    }

    // 测试目的：验证下一周期开盘价不是有限正数时会拒绝该行情。
    // 准备数据/过程：为当前周期和下一周期提供行情，并将下一周期 open 设置为 NaN。
    // 预期结果：返回 DataAlignmentError，并指出 next market open 无效。
    #[test]
    fn rejects_non_finite_next_market_open() {
        let positions = make_positions("time_stamp", vec![60_000_i64], vec![1_i64], vec![0.4]);
        let market = make_market_with_opens(vec![60_000_i64, 120_000], vec![1_i64, 1], vec![10.0_f64, f64::NAN], vec![11.0_f64, 12.0]);

        let error = match_position_market_data(&positions, &market, &HistoryInterval::OneMinute, |_| Ok(())).unwrap_err();

        assert!(matches!(&error, YuError::DataAlignmentError(message) if message.contains("next market open must be finite and positive")));
    }

    // 测试目的：验证持仓数据使用文档中的 timestamp 列名时仍可匹配行情。
    // 准备数据/过程：构造 timestamp 持仓列和具有相同 window_end、inst_id 的单条行情。
    // 预期结果：返回包含匹配时间戳、instrument 和 close 的数据。
    #[test]
    fn matches_timestamp_alias() {
        let positions = make_positions("timestamp", vec![100_i64], vec![1_i64], vec![-0.5]);
        let market = make_market(vec![100_i64], vec![1_i64], vec![11.0_f64]);

        let mut matched = Vec::new();
        match_position_market_data(&positions, &market, &HistoryInterval::OneMinute, |group| {
            matched.push(group);
            Ok(())
        })
        .unwrap();

        assert_eq!(matched[0].timestamp, 100);
        assert_eq!(matched[0].value[0].inst_id, 1);
        assert_eq!(matched[0].value[0].position_ratio, decimal("-0.5"));
        assert_eq!(matched[0].value[0].close, decimal("11"));
        assert_eq!(matched[0].value[0].trade_price, None);
        assert_eq!(matched[0].value[0].next_close, None);
    }

    // 测试目的：验证时间戳处理回调失败时，错误会从匹配函数传播给调用方。
    // 准备数据/过程：构造一条有效持仓和行情，并让处理回调返回自定义错误。
    // 预期结果：匹配函数返回回调中的错误，而不是吞掉错误继续执行。
    #[test]
    fn propagates_timestamp_processor_error() {
        let positions = make_positions("time_stamp", vec![100_i64], vec![1_i64], vec![0.1]);
        let market = make_market(vec![100_i64], vec![1_i64], vec![11.0_f64]);

        let error = match_position_market_data(&positions, &market, &HistoryInterval::OneMinute, |_| {
            Err(YuError::new("timestamp processor failed"))
        })
        .unwrap_err();

        assert!(matches!(&error, YuError::CustomError(message) if message == "timestamp processor failed"));
    }

    // 测试目的：验证对应时间戳和 instrument 的行情不存在时显式返回错误。
    // 准备数据/过程：构造一个持仓键及时间戳不同的行情行，再调用匹配函数。
    // 预期结果：错误信息指出缺少的 instrument 和时间戳。
    #[test]
    fn returns_error_when_matching_market_data_is_missing() {
        let positions = make_positions("time_stamp", vec![100_i64], vec![1_i64], vec![0.1]);
        let market = make_market(vec![200_i64], vec![1_i64], vec![11.0_f64]);

        let error = match_position_market_data(&positions, &market, &HistoryInterval::OneMinute, |_| Ok(())).unwrap_err();

        assert!(matches!(&error, YuError::DataAlignmentError(_)));
        assert!(error.to_string().contains("inst_id 1 at 100"));
    }

    // 测试目的：验证同一行情键存在多条记录时不会任意选取其中一条。
    // 准备数据/过程：构造一个持仓键和两条具有相同 window_end、inst_id 的行情后调用匹配函数。
    // 预期结果：返回指出重复行情键的错误。
    #[test]
    fn returns_error_when_market_key_is_duplicated() {
        let positions = make_positions("time_stamp", vec![100_i64], vec![1_i64], vec![0.1]);
        let market = make_market(vec![100_i64, 100], vec![1_i64, 1], vec![11.0_f64, 12.0]);

        let error = match_position_market_data(&positions, &market, &HistoryInterval::OneMinute, |_| Ok(())).unwrap_err();

        assert!(matches!(&error, YuError::DataAlignmentError(_)));
        assert!(error.to_string().contains("duplicate market data"));
    }

    // 测试目的：验证非有限的仓位比例作为对齐输入错误处理。
    // 准备数据/过程：构造包含 NaN position_ratio 的持仓数据和有效行情后调用匹配函数。
    // 预期结果：返回 DataAlignmentError，并指出仓位比例必须有限。
    #[test]
    fn rejects_non_finite_position_ratio() {
        let positions = make_positions("time_stamp", vec![100_i64], vec![1_i64], vec![f64::NAN]);
        let market = make_market(vec![100_i64], vec![1_i64], vec![11.0_f64]);

        let error = match_position_market_data(&positions, &market, &HistoryInterval::OneMinute, |_| Ok(())).unwrap_err();

        assert!(matches!(&error, YuError::DataAlignmentError(message) if message.contains("position_ratio must be finite")));
    }

    // 测试目的：验证有限但超出 Decimal 表示范围的仓位比例会显式报错。
    // 准备数据/过程：使用 f64::MAX 作为仓位比例，并提供有效行情后执行匹配。
    // 预期结果：返回包含字段名和行号的 DataAlignmentError。
    #[test]
    fn rejects_position_ratio_outside_decimal_range() {
        let positions = make_positions("time_stamp", vec![100_i64], vec![1_i64], vec![f64::MAX]);
        let market = make_market(vec![100_i64], vec![1_i64], vec![11.0_f64]);

        let error = match_position_market_data(&positions, &market, &HistoryInterval::OneMinute, |_| Ok(())).unwrap_err();

        assert!(
            matches!(&error, YuError::DataAlignmentError(message) if message.contains("position_ratio cannot be represented as Decimal at row 0"))
        );
    }
}
