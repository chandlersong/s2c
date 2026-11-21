//!
//! 在本mod中。想要作为一个回测结果的trait。
//! 在构思的时候，我想做成Sequence为主的模式。
//!
//! 主要原因是这个算是比较核心的一个模块。最好代码共享。而batch，本质是Sequence的一个组合。
//! 除非用polar这类作用，Batch也没有任何优化
//!
//! 本mod计算的是通过仓位的占比来计算整体的收益。也就是说，会不断的去rebalance仓位持有的各个的instrument。
//! 不考虑也不支持任何的固定的某个instrument的固定持仓的情况。计划中，这类情况应该在其他的mod实现。
//! 主要的原因还是，这类情况相对来适用性最广。因为这种可以去模拟那种固定持仓的情况。
//!
//! 而且觉得，这里的仓位调整，应该在策略方面，而不应该在这里。
//!
//!

use crate::backtest::core::ReturnCalculateTrait;
use crate::errors::YuError;
use crate::models::BacktestInst;
use li::tools::time::UnixTimeStamp;
use polars::prelude::{DataFrame, IntoLazy, JoinArgs, JoinType, SortMultipleOptions, col, lit};
use rust_decimal::dec;
use rust_decimal::prelude::FromPrimitive;
use std::collections::{HashMap, HashSet};
use yue::models::{Decimal, HistoryInterval};

#[derive(Clone)]
pub enum OrderReason {
    ReBalance,
    PositionChange,
}

#[derive(Clone)]
pub struct OrderInfo {
    inst_id: u64,
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
    position_target: Decimal,
    close: Decimal,
    trade_price: Option<Decimal>,
    next_close: Option<Decimal>,
    funding_fee: Option<Decimal>, //永续合约的用，其他一律为None
}

#[derive(Debug, PartialEq)]
pub struct TimestampPositionTarget {
    timestamp: i64,
    value: Vec<PositionTarget>,
}

fn decimal_from_f64(value: f64, field: &str, row: usize) -> Result<Decimal, YuError> {
    Decimal::from_f64(value).ok_or_else(|| YuError::DataError(format!("{field} cannot be represented as Decimal at row {row}")))
}

///
/// TODO: 这个函数的逻辑有点复杂。需要拆分成几个小函数。然后加上注释
/// 就是返回dataframe。然后在外部组变成相应的对象。相对灵活
///
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
        .map_err(|error| YuError::DataError(error.to_string()))?;
    let position_inst_ids = position_data
        .column("inst_id")
        .and_then(|column| column.i64())
        .map_err(|error| YuError::DataError(error.to_string()))?;
    let mut position_keys = HashSet::with_capacity(position_data.height());
    for row in 0..position_data.height() {
        let timestamp = position_timestamps
            .get(row)
            .ok_or_else(|| YuError::DataError(format!("null {position_timestamp_column} at row {row}")))?;
        let inst_id = position_inst_ids
            .get(row)
            .ok_or_else(|| YuError::DataError(format!("null position inst_id at row {row}")))?;
        if !position_keys.insert((timestamp, inst_id)) {
            return Err(YuError::DataError(format!(
                "duplicate position key for timestamp {timestamp} and inst_id {inst_id}"
            )));
        }
    }

    let interval_ms = i64::try_from(tick_interval.to_milliseconds()).map_err(|_| YuError::DataError("tick interval is too large".to_string()))?;
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
        .map_err(|error| YuError::DataError(error.to_string()))?
        .sort(["timestamp", "inst_id"], SortMultipleOptions::default())
        .map_err(|error| YuError::DataError(error.to_string()))?;

    if joined.height() > position_data.height() {
        return Err(YuError::DataError("duplicate market data matched to position rows".to_string()));
    }

    let timestamps = joined
        .column("timestamp")
        .and_then(|column| column.i64())
        .map_err(|error| YuError::DataError(error.to_string()))?;
    let inst_ids = joined
        .column("inst_id")
        .and_then(|column| column.i64())
        .map_err(|error| YuError::DataError(error.to_string()))?;
    let position_ratios = joined
        .column("position_ratio")
        .and_then(|column| column.f64())
        .map_err(|error| YuError::DataError(error.to_string()))?;
    let closes = joined
        .column("close")
        .and_then(|column| column.f64())
        .map_err(|error| YuError::DataError(error.to_string()))?;
    let trade_prices = joined
        .column("trade_price")
        .and_then(|column| column.f64())
        .map_err(|error| YuError::DataError(error.to_string()))?;
    let next_closes = joined
        .column("next_close")
        .and_then(|column| column.f64())
        .map_err(|error| YuError::DataError(error.to_string()))?;
    let mut current_timestamp = None;
    let mut current_positions = Vec::new();

    for row in 0..joined.height() {
        let timestamp = timestamps
            .get(row)
            .ok_or_else(|| YuError::DataError(format!("null {position_timestamp_column} at row {row}")))?;
        let inst_id = inst_ids
            .get(row)
            .ok_or_else(|| YuError::DataError(format!("null position inst_id at row {row}")))?;
        let inst_id = u64::try_from(inst_id).map_err(|_| YuError::DataError(format!("negative position inst_id at row {row}")))?;
        let position_ratio = position_ratios
            .get(row)
            .ok_or_else(|| YuError::DataError(format!("null position_ratio at row {row}")))?;
        if !position_ratio.is_finite() {
            return Err(YuError::DataError(format!("position_ratio must be finite at row {row}")));
        }
        let position_ratio = decimal_from_f64(position_ratio, "position_ratio", row)?;
        let close = closes
            .get(row)
            .ok_or_else(|| YuError::DataError(format!("market data not found for inst_id {inst_id} at {timestamp}")))?;
        if !close.is_finite() || close <= 0.0 {
            return Err(YuError::DataError(format!("market close must be finite and positive at row {row}")));
        }
        let close = decimal_from_f64(close, "close", row)?;
        let trade_price = trade_prices.get(row);
        if trade_price.is_some_and(|price| !price.is_finite() || price <= 0.0) {
            return Err(YuError::DataError(format!("next market open must be finite and positive at row {row}")));
        }
        let trade_price = trade_price.map(|price| decimal_from_f64(price, "trade_price", row)).transpose()?;
        let next_close = next_closes.get(row);
        if next_close.is_some_and(|price| !price.is_finite() || price <= 0.0) {
            return Err(YuError::DataError(format!("next market close must be finite and positive at row {row}")));
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
pub struct PNL {
    pub realized_pnl: Decimal,
    pub unrealized_pnl: Decimal,
    /// 当前周期资金费用，不提前计入下一周期。
    pub funding_fee: Decimal,
    pub commission: Decimal,
    pub hold_value: Decimal,
}

///
/// 预计的收益和订单信息
///
#[derive(Clone)]
pub struct ReadyPosChg {
    pub pnl: PNL,
    pub order_info: OrderInfo,
}

#[derive(Clone)]
pub struct SequencePosition<'p> {
    position_state: &'p HashMap<u64, BacktestPosition>,
    instruments: &'p HashMap<u64, BacktestInst>,
}

///
/// 计算一个instrument在一个tick中因为仓位变化，而产生的收益和order。
/// - 不涉及仓位的变化。仓位的变化，应该在其调用的地方去管理。
///     - 外部的仓位变化，应该根据这里的PNL的变化来定义。
/// - 产生的订单。在使用时，更偏向于日志，或者实盘时候，发给执行模块去执行的。
///     - 这里其实我也没有想好，因为这样会产生很多rebalance的小订单。但是分析的时候是没用的。现在的初步设想是分析订单放入策略模块。
///
/// # 数据计算
/// 1. realized_position: 本次订单平掉的数量，计算方式为当前仓位与反向订单量绝对值的较小值。
/// 2. realized_pnl: 已实现盈亏，计算公式为：(trade_price - entry_price) * realized_position * 当前仓位方向。
/// 3. trade_amount: 本次交易金额，计算公式为：trade_price * order_amount.abs()。
/// 4. unrealized_pnl: 未实现盈亏，计算公式为：(close - 新持仓均价) * 新持仓数量。
/// 5. hold_value: 持仓价值，计算公式为：close * 新持仓数量.abs()。
/// 6. commission: 手续费，计算公式为：trade_amount * c_rate
/// # 基本流程
///
/// 1. 将 position_target 作为有符号目标数量（负数代表做空）。
/// 2. 根据当前持仓与目标数量的差值计算订单，并处理最小下单单位和最小名义金额。
/// 3. 按成交方向拆分已平仓数量，计算已实现/未实现盈亏、费用和持仓价值。
/// 4. 返回 PNL 汇总及包含新持仓状态的 OrderInfo。
/// OrderInfo 只记录本次订单，不承担持仓状态的传递。
///
///
/// # 规则
/// 1. instrument的下架
///     - target中的trade_price或者next_close为None，说明下架了
///     - 下架的instrument，以target的close的价格来平仓
/// 2. 数量按 instrument 的最小交易数量取最近档位，小于最小名义金额的非零目标单不下单。
/// 3. 当前周期资金费按交易前仓位和当前收盘价计算；手续费沿用 Python cal_one_symbol 的计算方式。
/// 5. next_close 用于确认下一周期行情有效；不用于当前tick的任何计算。
///
/// # 参数与中间值
/// - `target`: 本 tick 的目标数量及当前/下一周期市场数据。
/// - `position`: 交易前仓位数量和入场均价。
/// - `instruments`: 最小下单数量和最小名义金额规则。
/// - `c_rate`: 本次成交的手续费率。
/// - `min_trade_quantity` / `min_trade_size`: 数量精度和名义金额限制。
/// - `order_amount` / `trade_price` / `order_reason`: 本次订单数量、成交价和原因。
/// - `raw_order_amount` / `rounded_order`: 舍入前后的目标持仓差值。
/// - `order_value` / `commission`: 本次订单按成交价计算的名义金额及手续费。
/// - `current_position` / `new_position`: 交易前后的仓位数量。
/// - `realized_position` / `position_direction` / `realized_pnl`: 平仓数量、原仓位方向及已实现盈亏。
/// - `next_close`: 用于确认下一周期行情有效；不用于当前tick的任何计算。
/// - `funding_rate` / `funding_fee`: 当前周期费率及按本 tick 仓位计算的资金费用。
/// - `new_entry_price` / `unrealized_pnl`: 新仓位均价及未实现盈亏。
/// - `hold_value`: 当前持仓的名义价值。
///
fn one_instrument_one_tick(
    target: &PositionTarget,
    position: &BacktestPosition,
    instruments: &BacktestInst,
    c_rate: Decimal,
) -> Result<ReadyPosChg, YuError> {
    // 获取 instrument 的数量精度和最小名义金额限制。
    let min_trade_quantity = instruments.min_trade_quantity();
    let min_trade_size = instruments.min_trade_size();

    // 正常情况下按下一根 K 线成交；价格缺失或无效时，沿用引擎规则按当前收盘价强平。
    // 统一确定本次订单数量、成交价和订单原因，避免后续重复判断下架状态。
    let (order_amount, trade_price, order_reason) = match (target.trade_price, target.next_close) {
        (Some(trade_price), Some(next_close)) if trade_price > Decimal::ZERO && next_close > Decimal::ZERO => {
            // position_target 是目标数量，订单是目标数量减去当前持仓。
            let raw_order_amount = target.position_target - position.position;
            // 将订单数量就近舍入到 instrument 允许的数量档位。
            let rounded_order = (raw_order_amount / min_trade_quantity)
                .round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointNearestEven)
                * min_trade_quantity;
            // 非零目标订单低于最小名义金额时跳过；目标为零时仍允许平仓。
            let order_amount = if rounded_order.abs() * target.close < min_trade_size && !target.position_target.is_zero() {
                Decimal::ZERO
            } else {
                rounded_order
            };
            (order_amount, trade_price, OrderReason::ReBalance)
        }
        _ => (-position.position, target.close, OrderReason::PositionChange),
    };

    // 按实际成交价计算订单金额，用于计算手续费。
    let order_value = order_amount.abs() * trade_price;
    // 保存交易前仓位，用于计算平仓盈亏。
    let current_position = position.position;

    // 只有反向订单会平掉已有仓位；开仓和同向加仓不产生已实现仓位。
    let realized_position = if order_amount * current_position < Decimal::ZERO {
        order_amount.abs().min(current_position.abs())
    } else {
        Decimal::ZERO
    };
    // 空仓时该方向不会影响盈亏，因为 realized_position 为零。
    let position_direction = if current_position < Decimal::ZERO {
        Decimal::NEGATIVE_ONE
    } else {
        Decimal::ONE
    };
    // 已实现盈亏仅使用本次被平掉的数量。
    let realized_pnl = realized_position * (trade_price - position.entry_price) * position_direction;
    // 交易后仓位数量等于旧仓位加本次订单量。
    let new_position = current_position + order_amount;

    // 当前周期资金费按本 tick 调整后的仓位和当前 close 计算，不预提下一周期费用。
    let funding_fee = target.funding_fee.map_or(Decimal::ZERO, |rate| -new_position * target.close * rate);

    // 加仓按数量加权；减仓保留旧均价；完全反手时新仓均价从成交价开始。
    let new_entry_price = if new_position.is_zero() {
        Decimal::ZERO
    } else if realized_position.is_zero() {
        (current_position * position.entry_price + order_amount * trade_price) / new_position
    } else if order_amount.abs() >= current_position.abs() {
        trade_price
    } else {
        position.entry_price
    };
    // 按当前周期收盘价计算未实现盈亏和持仓价值。
    let unrealized_pnl = (target.close - new_entry_price) * new_position;

    // 持仓价值按当前收盘价计算多空总敞口。
    let hold_value = new_position.abs() * target.close;
    // 手续费和成交金额均按本次实际成交价计算。
    let commission = order_value * c_rate;

    Ok(ReadyPosChg {
        pnl: PNL {
            realized_pnl,
            unrealized_pnl,
            funding_fee,
            commission,
            hold_value,
        },
        order_info: OrderInfo {
            inst_id: target.inst_id,
            order_amount,
            unrealized_pnl,
            realized_pnl,
            // OrderInfo描述本次成交，不承载账户持仓的加权入场价。
            entry_price: trade_price,
            order_reason,
        },
    })
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
    use crate::models::BacktestInstTrait;

    struct TestInstrument {
        min_trade_size: Decimal,
        min_trade_quantity: Decimal,
    }

    impl BacktestInstTrait for TestInstrument {
        fn inst_id(&self) -> u64 {
            7
        }

        fn symbol(&self) -> &str {
            "BTCUSDT"
        }

        fn status(&self) -> &str {
            "TRADING"
        }

        fn base_asset(&self) -> &str {
            "BTC"
        }

        fn quote_asset(&self) -> &str {
            "USDT"
        }

        fn instrument_type(&self) -> &str {
            "binance_SWAP"
        }

        fn min_trade_size(&self) -> Decimal {
            self.min_trade_size
        }

        fn min_trade_quantity(&self) -> Decimal {
            self.min_trade_quantity
        }

        fn tick_size(&self) -> Decimal {
            dec!(0.01)
        }
    }

    fn instrument(min_trade_size: Decimal, min_trade_quantity: Decimal) -> BacktestInst {
        Box::new(TestInstrument {
            min_trade_size,
            min_trade_quantity,
        })
    }

    // 测试目的：验证 position_target 直接作为目标数量，并汇总交易盈亏、资金费和手续费。
    // 准备数据/过程：目标数量为 10，设置下一周期价格与资金费率后执行计算。
    // 预期结果：目标下单量为 10；开仓的 realized_pnl 为零，当期资金费为 -10，手续费单独记录。
    #[test]
    fn calculates_position_and_pnl_for_a_regular_tick() {
        let target = PositionTarget {
            inst_id: 7,
            position_target: Decimal::from(10),
            close: Decimal::from(100),
            trade_price: Some(Decimal::from(110)),
            next_close: Some(Decimal::from(120)),
            funding_fee: Some(dec!(0.01)),
        };
        let position = BacktestPosition {
            position: Decimal::ZERO,
            entry_price: Decimal::ZERO,
        };
        let instrument = instrument(Decimal::from(5), dec!(0.1));

        let result = one_instrument_one_tick(&target, &position, &instrument, dec!(0.001)).unwrap();

        assert_eq!(result.order_info.order_amount, Decimal::from(10));
        assert_eq!(result.order_info.entry_price, Decimal::from(110));
        assert_eq!(result.pnl.realized_pnl, Decimal::ZERO);
        assert_eq!(result.pnl.funding_fee, Decimal::from(-10));
        assert_eq!(result.pnl.unrealized_pnl, Decimal::from(-100));
        assert_eq!(result.pnl.commission, dec!(1.1));
        assert_eq!(result.pnl.hold_value, Decimal::from(1000));
    }

    // 测试目的：验证资金费只按当前周期已有仓位计算，不提前使用新仓位和下一周期收盘价。
    // 准备数据/过程：保留 2 单位多仓，当前 close 为 100、下一 close 为 120，资金费率为 1%。
    // 预期结果：当前周期资金费用为 -2，且不计入 realized_pnl。
    #[test]
    fn calculates_funding_fee_for_current_period_position() {
        // 当前 tick 的仓位目标与已有仓位相同，不产生下单量。
        let target = PositionTarget {
            inst_id: 7,
            position_target: Decimal::from(2),
            close: Decimal::from(100),
            trade_price: Some(Decimal::from(110)),
            next_close: Some(Decimal::from(120)),
            funding_fee: Some(dec!(0.01)),
        };
        // 交易前已有 2 单位多仓，资金费用按此仓位计提。
        let position = BacktestPosition {
            position: Decimal::from(2),
            entry_price: Decimal::from(90),
        };
        let instrument = instrument(Decimal::from(5), dec!(0.1));

        let result = one_instrument_one_tick(&target, &position, &instrument, dec!(0.001)).unwrap();

        assert_eq!(result.order_info.order_amount, Decimal::ZERO);
        assert_eq!(result.pnl.realized_pnl, Decimal::ZERO);
        assert_eq!(result.pnl.funding_fee, Decimal::from(-2));
    }

    // 测试目的：验证 OrderInfo 的 entry_price 记录本次订单成交价，而不是持仓加权入场价。
    // 准备数据/过程：已有 1 单位、入场价 90 的多仓，目标调整到 2 单位，并以 110 成交。
    // 预期结果：OrderInfo.entry_price 为 110；未实现盈亏按当前收盘价与调整后的持仓均价计算。
    #[test]
    fn order_info_uses_this_tick_trade_price() {
        let target = PositionTarget {
            inst_id: 7,
            position_target: Decimal::from(2),
            close: Decimal::from(105),
            trade_price: Some(Decimal::from(110)),
            next_close: Some(Decimal::from(120)),
            funding_fee: None,
        };
        let position = BacktestPosition {
            position: Decimal::ONE,
            entry_price: Decimal::from(90),
        };
        let instrument = instrument(Decimal::from(5), dec!(0.1));

        let result = one_instrument_one_tick(&target, &position, &instrument, dec!(0.001)).unwrap();

        assert_eq!(result.order_info.order_amount, Decimal::ONE);
        assert_eq!(result.order_info.entry_price, Decimal::from(110));
        assert_eq!(result.pnl.unrealized_pnl, Decimal::from(10));
    }

    // 测试目的：验证缺失下一周期价格时按当前收盘价强制平仓。
    // 准备数据/过程：构造 2 单位多仓、90 的入场价及缺失的下一周期成交价，调用单 instrument tick 计算。
    // 预期结果：下单量为 -2，按当前收盘价实现盈亏并收取手续费，仓位归零。
    #[test]
    fn closes_position_at_current_close_when_next_market_data_is_missing() {
        let target = PositionTarget {
            inst_id: 7,
            position_target: Decimal::ONE,
            close: Decimal::from(100),
            trade_price: None,
            next_close: Some(Decimal::from(105)),
            funding_fee: None,
        };
        let position = BacktestPosition {
            position: Decimal::from(2),
            entry_price: Decimal::from(90),
        };
        let instrument = instrument(Decimal::from(5), dec!(0.1));

        let result = one_instrument_one_tick(&target, &position, &instrument, dec!(0.001)).unwrap();

        assert_eq!(result.order_info.order_amount, Decimal::from(-2));
        assert_eq!(result.order_info.entry_price, Decimal::from(100));
        assert_eq!(result.pnl.realized_pnl, Decimal::from(20));
        assert_eq!(result.pnl.unrealized_pnl, Decimal::ZERO);
        assert_eq!(result.pnl.commission, dec!(0.2));
        assert!(matches!(result.order_info.order_reason, OrderReason::PositionChange));
    }

    // 测试目的：验证低于最小名义下单金额的非零目标订单会被取消。
    // 准备数据/过程：使用小目标仓位使舍入后的下单金额低于 instrument 的最小下单金额。
    // 预期结果：订单量和手续费均为零，持仓维持不变。
    #[test]
    fn skips_orders_below_minimum_trade_size() {
        let target = PositionTarget {
            inst_id: 7,
            position_target: dec!(0.00001),
            close: Decimal::from(100),
            trade_price: Some(Decimal::from(100)),
            next_close: Some(Decimal::from(100)),
            funding_fee: None,
        };
        let position = BacktestPosition {
            position: Decimal::ZERO,
            entry_price: Decimal::ZERO,
        };
        let instrument = instrument(Decimal::from(5), dec!(0.001));

        let result = one_instrument_one_tick(&target, &position, &instrument, dec!(0.001)).unwrap();

        assert_eq!(result.order_info.order_amount, Decimal::ZERO);
        assert_eq!(result.pnl.commission, Decimal::ZERO);
    }

    // 测试目的：验证从空头反手为多头时，订单先平空再开多，并按旧空仓方向计算已实现盈亏。
    // 准备数据/过程：交易前持有 2 单位空仓、入场价 100，目标多仓为 3，成交价为 110。
    // 预期结果：订单为买入 5 单位，已实现盈亏为 -20，新多仓未实现盈亏为 -30。
    #[test]
    fn reverses_short_position_to_long() {
        // 目标多仓数量和当前周期市场价格。
        let target = PositionTarget {
            inst_id: 7,
            position_target: Decimal::from(3),
            close: Decimal::from(100),
            trade_price: Some(Decimal::from(110)),
            next_close: Some(Decimal::from(120)),
            funding_fee: None,
        };
        // 交易前的空仓数量和入场均价。
        let position = BacktestPosition {
            position: Decimal::from(-2),
            entry_price: Decimal::from(100),
        };
        // instrument 的最小名义金额和数量档位。
        let instrument = instrument(Decimal::from(5), dec!(0.1));

        // 按目标仓位差计算本次反手订单及其 PNL。
        let result = one_instrument_one_tick(&target, &position, &instrument, dec!(0.001)).unwrap();

        assert_eq!(result.order_info.order_amount, Decimal::from(5));
        assert_eq!(result.pnl.realized_pnl, Decimal::from(-20));
        assert_eq!(result.pnl.unrealized_pnl, Decimal::from(-30));
        assert_eq!(result.order_info.entry_price, Decimal::from(110));
    }

    // 测试目的：验证从空仓直接建立纯空头时，开仓不产生已实现盈亏。
    // 准备数据/过程：以零仓位为起点，目标空仓为 -3，成交价为 110，当前 close 为 100。
    // 预期结果：订单和新仓位均为 -3，已实现盈亏为零，未实现盈亏为正 30。
    #[test]
    fn opens_a_pure_short_position() {
        // 目标空仓数量和当前周期市场价格。
        let target = PositionTarget {
            inst_id: 7,
            position_target: Decimal::from(-3),
            close: Decimal::from(100),
            trade_price: Some(Decimal::from(110)),
            next_close: Some(Decimal::from(120)),
            funding_fee: None,
        };
        // 从零仓位开始，不存在可实现的旧仓盈亏。
        let position = BacktestPosition {
            position: Decimal::ZERO,
            entry_price: Decimal::ZERO,
        };
        // instrument 的最小名义金额和数量档位。
        let instrument = instrument(Decimal::from(5), dec!(0.1));

        // 按负目标数量开空并计算成交后的 PNL。
        let result = one_instrument_one_tick(&target, &position, &instrument, dec!(0.001)).unwrap();

        assert_eq!(result.order_info.order_amount, Decimal::from(-3));
        assert_eq!(result.pnl.realized_pnl, Decimal::ZERO);
        assert_eq!(result.pnl.unrealized_pnl, Decimal::from(30));
        assert_eq!(result.order_info.entry_price, Decimal::from(110));
    }
}
