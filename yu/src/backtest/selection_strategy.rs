//!
//! 在一个范围内，选择一个池子的策略。
//! 这个策略的目标是，选择一个池子里面的选择一些标的去选择去持有的策略。
//!
//!
//!

use crate::backtest::core::{BatchDataFeed, EngineTrait};
use crate::errors::YuError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use log::{debug, info};
use polars::prelude::DataFrame;
use std::collections::BTreeMap;
use std::sync::Arc;

pub fn position_summary_lines(positions: &DataFrame) -> Result<Vec<String>, YuError> {
    let time_stamps = positions.column("time_stamp")?.i64()?;
    let inst_ids = positions.column("inst_id")?.i64()?;
    let position_ratios = positions.column("position_ratio")?.f64()?;
    let mut positions_by_time: BTreeMap<i64, Vec<(i64, f64)>> = BTreeMap::new();

    for row in 0..positions.height() {
        let time_stamp = time_stamps
            .get(row)
            .ok_or_else(|| YuError::new(&format!("null time_stamp at row {row}")))?;
        let inst_id = inst_ids.get(row).ok_or_else(|| YuError::new(&format!("null inst_id at row {row}")))?;
        let position_ratio = position_ratios
            .get(row)
            .ok_or_else(|| YuError::new(&format!("null position_ratio at row {row}")))?;
        positions_by_time.entry(time_stamp).or_default().push((inst_id, position_ratio));
    }

    positions_by_time
        .into_iter()
        .map(|(time_stamp, positions)| {
            let date_time = DateTime::<Utc>::from_timestamp_millis(time_stamp)
                .ok_or_else(|| YuError::new(&format!("time_stamp is out of range: {time_stamp}")))?;
            let positions = positions
                .into_iter()
                .map(|(inst_id, ratio)| format!("inst_id={inst_id}, position_ratio={ratio}"))
                .collect::<Vec<_>>()
                .join("; ");
            Ok(format!("{}: {positions}", date_time.format("%Y-%m-%d %H:%M:%S%.3f UTC")))
        })
        .collect()
}

#[async_trait]
pub trait BatchSelectStrategyTrait {
    ///
    /// 返回的是一个position的DataFrame，里面包含了选择的标的和笔记持仓比例
    ///
    async fn select(&self, klines: &DataFrame) -> Result<DataFrame, YuError>;

    ///
    /// 方法select的参数，kline必须要有的列。可以多，但是不能少。
    ///
    fn must_existing_columns(&self) -> Vec<String>;
}

pub type BatchSelectionStrategy = Arc<dyn BatchSelectStrategyTrait + Send + Sync>;

pub struct SelectionConfig {
    long_num: usize,  //做多的数量
    short_num: usize, //做空的数量
}

impl SelectionConfig {
    pub fn new(long_num: usize, short_num: usize) -> Self {
        SelectionConfig { long_num, short_num }
    }
}

pub struct BatchSelectionEngine {
    data_feed: BatchDataFeed,
    strategy: BatchSelectionStrategy,
}

impl BatchSelectionEngine {
    pub fn new(data_feed: BatchDataFeed, strategy: BatchSelectionStrategy) -> Self {
        BatchSelectionEngine { data_feed, strategy }
    }
}

#[async_trait]
impl EngineTrait for BatchSelectionEngine {
    type Result = DataFrame;

    async fn execute(&self) -> Result<Self::Result, YuError> {
        let source_data = self.data_feed.get_all_data().await?;
        debug!(
            "BatchSelection Engine source data shape: {:?},memory usage: {:.2} MB",
            source_data.shape(),
            source_data.estimated_size() as f64 / 1024.0 / 1024.0
        );
        let selected_df = self.strategy.select(&source_data).await?;
        for line in position_summary_lines(&selected_df)? {
            info!("BatchSelection positions {line}");
        }
        Ok(selected_df)
    }
}

pub mod demo {
    use crate::backtest::selection_strategy::{BatchSelectionStrategy, SelectionConfig};
    use crate::errors::YuError;
    use async_trait::async_trait;
    use polars::prelude::{DataFrame, NamedFrom, Series};
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Arc;

    const LOOKBACK_MS: i64 = 24 * 60 * 60 * 1_000;

    pub struct SelectionDemoStrategy {
        config: SelectionConfig,
    }

    impl SelectionDemoStrategy {
        pub fn new_for_demo() -> BatchSelectionStrategy {
            Arc::new(SelectionDemoStrategy {
                config: SelectionConfig::new(3, 3),
            })
        }
    }

    #[cfg(test)]
    mod position_summary_tests {
        use super::*;
        use polars::prelude::{NamedFrom, Series};

        // 测试目的：验证相同时间戳的持仓合并打印，且不同时间戳按先后分行。
        // 准备数据/过程：构造两个时间戳、三条持仓的DataFrame并生成摘要行。
        // 预期结果：相同时间戳的inst_id和position_ratio出现在同一行，另一个时间戳单独一行。
        #[test]
        fn groups_position_summaries_by_timestamp() {
            let positions = DataFrame::new(vec![
                Series::new("time_stamp".into(), vec![0_i64, 0, 3_600_000]).into(),
                Series::new("inst_id".into(), vec![11_i64, 22, 33]).into(),
                Series::new("position_ratio".into(), vec![0.25_f64, -0.5, 0.25]).into(),
                Series::new("direction".into(), vec![1_i32, -1, 1]).into(),
            ])
            .unwrap();

            let lines = super::super::position_summary_lines(&positions).unwrap();

            assert_eq!(
                lines,
                vec![
                    "1970-01-01 00:00:00.000 UTC: inst_id=11, position_ratio=0.25; inst_id=22, position_ratio=-0.5",
                    "1970-01-01 01:00:00.000 UTC: inst_id=33, position_ratio=0.25",
                ]
            );
        }

        // 测试目的：验证摘要DataFrame缺少必要列时不会静默跳过打印。
        // 准备数据/过程：构造只有time_stamp列的DataFrame并生成摘要行。
        // 预期结果：返回包含缺失inst_id列信息的错误。
        #[test]
        fn position_summary_returns_error_when_column_is_missing() {
            let positions = DataFrame::new(vec![Series::new("time_stamp".into(), vec![0_i64]).into()]).unwrap();

            let error = super::super::position_summary_lines(&positions).unwrap_err();

            assert!(error.to_string().contains("inst_id"));
        }
    }

    #[async_trait]
    impl super::BatchSelectStrategyTrait for SelectionDemoStrategy {
        ///
        /// 返回的是一个position的DataFrame，里面包含了选择的标的和笔记持仓比例
        /// 输入值 klines的columns为must_existing_columns()返回。不用校验。
        /// 返回值为 time_stamp, inst_id, position_ratio, direction
        /// timestamp: 应该取对应的window_end，因为读取的数据是之前一个tick的数据。
        /// inst_id: 选择的标的
        /// position_ratio: 选择的标的的仓位比例，做多为正，做空为负
        /// direction: 仓位类型，做多为1，做空为-1
        ///
        /// ## 流程
        /// 1. 计算每个标的过去24小时的涨跌幅，
        /// 2. 在每个时间段选出涨幅最大的long_num个标的，和跌幅最大的short_num个标的。
        /// 3. 平均分配仓位比例，做多为0.5/long_num，做空为-0.5/short_num
        /// 4， 返回结果
        ///
        async fn select(&self, klines: &DataFrame) -> Result<DataFrame, YuError> {
            let inst_ids = klines.column("inst_id")?.i64()?;
            let candle_begin_times = klines.column("candle_begin_time")?.i64()?;
            let window_ends = klines.column("window_end")?.i64()?;
            let closes = klines.column("close")?.f64()?;

            let mut prices_by_time: HashMap<(i64, i64), f64> = HashMap::with_capacity(klines.height());
            let mut points = Vec::with_capacity(klines.height());

            for row in 0..klines.height() {
                let inst_id = inst_ids.get(row).ok_or_else(|| YuError::new(&format!("null inst_id at row {row}")))?;
                let candle_begin_time = candle_begin_times
                    .get(row)
                    .ok_or_else(|| YuError::new(&format!("null candle_begin_time at row {row}")))?;
                let window_end = window_ends
                    .get(row)
                    .ok_or_else(|| YuError::new(&format!("null window_end at row {row}")))?;
                let close = closes.get(row).ok_or_else(|| YuError::new(&format!("null close at row {row}")))?;
                if !close.is_finite() || close <= 0.0 {
                    return Err(YuError::new(&format!("close must be finite and positive at row {row}")));
                }

                if prices_by_time.insert((inst_id, candle_begin_time), close).is_some() {
                    return Err(YuError::new(&format!("duplicate K-line for inst_id {inst_id} at {candle_begin_time}")));
                }
                points.push((inst_id, candle_begin_time, window_end, close));
            }

            let mut returns_by_window: BTreeMap<i64, Vec<(i64, f64)>> = BTreeMap::new();
            for (inst_id, candle_begin_time, window_end, close) in points {
                let Some(lookback_time) = candle_begin_time.checked_sub(LOOKBACK_MS) else {
                    continue;
                };
                let Some(previous_close) = prices_by_time.get(&(inst_id, lookback_time)) else {
                    continue;
                };
                let change = close / previous_close - 1.0;
                if !change.is_finite() {
                    return Err(YuError::new(&format!(
                        "24-hour return is not finite for inst_id {inst_id} at {candle_begin_time}"
                    )));
                }
                returns_by_window.entry(window_end).or_default().push((inst_id, change));
            }

            let mut time_stamps = Vec::new();
            let mut selected_inst_ids = Vec::new();
            let mut position_ratios = Vec::new();
            let mut directions = Vec::new();

            for (window_end, mut returns) in returns_by_window {
                if self.config.long_num > 0 {
                    returns.sort_by(|left, right| right.1.total_cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
                    let position_ratio = 0.5 / self.config.long_num as f64;
                    for (inst_id, _) in returns.iter().take(self.config.long_num) {
                        time_stamps.push(window_end);
                        selected_inst_ids.push(*inst_id);
                        position_ratios.push(position_ratio);
                        directions.push(1_i32);
                    }
                }

                if self.config.short_num > 0 {
                    returns.sort_by(|left, right| left.1.total_cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
                    let position_ratio = -0.5 / self.config.short_num as f64;
                    for (inst_id, _) in returns.iter().take(self.config.short_num) {
                        time_stamps.push(window_end);
                        selected_inst_ids.push(*inst_id);
                        position_ratios.push(position_ratio);
                        directions.push(-1_i32);
                    }
                }
            }

            Ok(DataFrame::new(vec![
                Series::new("time_stamp".into(), time_stamps).into(),
                Series::new("inst_id".into(), selected_inst_ids).into(),
                Series::new("position_ratio".into(), position_ratios).into(),
                Series::new("direction".into(), directions).into(),
            ])?)
        }

        fn must_existing_columns(&self) -> Vec<String> {
            vec![
                "id".to_string(),
                "inst_id".to_string(),
                "candle_begin_time".to_string(),
                "window_end".to_string(),
                "open".to_string(),
                "high".to_string(),
                "low".to_string(),
                "close".to_string(),
                "volume".to_string(),
                "quote_volume".to_string(),
                "number_of_trades".to_string(),
                "taker_buy_base_asset_volume".to_string(),
                "taker_buy_quote_asset_volume".to_string(),
                "close_time".to_string(),
            ]
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::backtest::selection_strategy::BatchSelectStrategyTrait;

        fn kline_data(inst_ids: Vec<i64>, times: Vec<i64>, closes: Vec<f64>) -> DataFrame {
            let window_ends = times.iter().map(|time| time + 3_600_000).collect::<Vec<_>>();
            DataFrame::new(vec![
                Series::new("inst_id".into(), inst_ids).into(),
                Series::new("candle_begin_time".into(), times).into(),
                Series::new("window_end".into(), window_ends).into(),
                Series::new("close".into(), closes).into(),
            ])
            .unwrap()
        }

        // 测试目的：验证按每个标的过去24小时收益率排序，并生成指定多空数量、权重和方向。
        // 准备数据/过程：构造三个标的在相隔24小时的收盘价，配置做多两个、做空一个并调用策略。
        // 预期结果：收益最高的两个标的做多、收益最低的标的做空，时间戳使用对应的 window_end。
        #[tokio::test]
        async fn select_ranks_24_hour_returns_and_assigns_positions() {
            let day = LOOKBACK_MS;
            let klines = kline_data(
                vec![1, 2, 3, 1, 2, 3],
                vec![0, 0, 0, day, day, day],
                vec![100.0, 100.0, 100.0, 120.0, 110.0, 90.0],
            );
            let strategy = SelectionDemoStrategy {
                config: SelectionConfig::new(2, 1),
            };

            let selected = strategy.select(&klines).await.unwrap();

            assert_eq!(selected.column("time_stamp").unwrap().i64().unwrap().get(0), Some(day + 3_600_000));
            assert_eq!(
                selected.column("inst_id").unwrap().i64().unwrap().into_no_null_iter().collect::<Vec<_>>(),
                vec![1, 2, 3]
            );
            assert_eq!(
                selected
                    .column("position_ratio")
                    .unwrap()
                    .f64()
                    .unwrap()
                    .into_no_null_iter()
                    .collect::<Vec<_>>(),
                vec![0.25, 0.25, -0.5]
            );
            assert_eq!(
                selected
                    .column("direction")
                    .unwrap()
                    .i32()
                    .unwrap()
                    .into_no_null_iter()
                    .collect::<Vec<_>>(),
                vec![1, 1, -1]
            );
        }

        // 测试目的：验证缺少24小时前价格的标的不会被错误地纳入排名。
        // 准备数据/过程：仅提供当前时点的有效K线，并配置做多一个、做空一个后调用策略。
        // 预期结果：返回包含规定列但没有持仓行的DataFrame。
        #[tokio::test]
        async fn select_skips_rows_without_24_hour_history() {
            let klines = kline_data(vec![1], vec![LOOKBACK_MS], vec![100.0]);
            let strategy = SelectionDemoStrategy {
                config: SelectionConfig::new(1, 1),
            };

            let selected = strategy.select(&klines).await.unwrap();

            assert_eq!(selected.height(), 0);
            assert_eq!(
                selected.get_column_names_str(),
                vec!["time_stamp", "inst_id", "position_ratio", "direction"]
            );
        }

        // 测试目的：验证缺少策略必需的K线列时返回明确错误。
        // 准备数据/过程：构造只有 inst_id 列的DataFrame并调用策略。
        // 预期结果：返回Polars列缺失错误，而不是产生不完整的持仓结果。
        #[tokio::test]
        async fn select_returns_error_when_required_column_is_missing() {
            let klines = DataFrame::new(vec![Series::new("inst_id".into(), vec![1_i64]).into()]).unwrap();
            let strategy = SelectionDemoStrategy {
                config: SelectionConfig::new(1, 1),
            };

            let error = strategy.select(&klines).await.unwrap_err();

            assert!(error.to_string().contains("candle_begin_time"));
        }
    }
}
