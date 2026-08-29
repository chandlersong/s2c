use crate::data_integrity::models::{RepairRequest, ValidationGap, ValidationResult};
use crate::errors::YuError;
use async_trait::async_trait;
use std::sync::Arc;
use yue::models::HistoryInterval;

/// 可插拔校验策略接口，Checker 调用实现校验逻辑。
#[async_trait]
pub trait ValidationStrategyTrait: Send + Sync {
    async fn validate(&self) -> Result<Option<ValidationResult>, YuError>;

    fn name(&self) -> String;
}

///
///
/// 因为这个逻辑，需要计算不同的数据。
/// 通过二分查找的方式，找出 start 和 end 之间的 gap。返回 RepairRequest。
///
/// 规则
/// 1. 如果 start 和 end 之间的数据数量小于等于 1，则直接返回空的 RepairRequest。
/// 2. 如果 start 和 end 之间的数据数量大于 1，则计算中间的 mid = (start + end) / 2，
///    计算 start 和 mid 之间的数据数量，以及 mid 和 end 之间的数据数量。
///
///
pub fn binary_search_gap(
    identify: &str,
    trade_type: &str,
    start: u64,
    end: u64,
    interval: &HistoryInterval,
    data_source: BinarySearchDDataSource,
) -> Result<Vec<RepairRequest>, YuError> {
    use yue::tools::get_snow_flake_id_u64;

    // defensive
    if start >= end {
        return Ok(Vec::new());
    }

    let interval_ms = interval.to_milliseconds();
    let expected = (end - start) / interval_ms;

    // count existing distinct slots in [start, end)
    let actual = data_source.count_distinct_between(start, end)?;
    if actual == expected {
        return Ok(Vec::new());
    }

    // base case: single slot missing
    if expected == 1 {
        let gap = ValidationGap::MissingData {
            symbol: identify.to_string(),
            trade_type: trade_type.to_string(),
            start_time: start,
            end_time: start + interval_ms,
            table: data_source.table_name(),
        };
        let req = RepairRequest {
            id: get_snow_flake_id_u64(),
            strategy: "binary_search_gap".to_string(),
            gaps: vec![gap],
        };
        return Ok(vec![req]);
    }

    // compute aligned mid
    let mid = start + ((end - start) / 2 / interval_ms) * interval_ms;

    // recurse left and right
    let mut left = binary_search_gap(identify, trade_type, start, mid, interval, data_source.clone())?;
    let mut right = binary_search_gap(identify, trade_type, mid, end, interval, data_source.clone())?;
    left.append(&mut right);
    let gaps: Vec<ValidationGap> = left.into_iter().flat_map(|req| req.gaps.into_iter()).collect();
    let gaps = merge_gaps(gaps);
    if gaps.is_empty() {
        return Ok(Vec::new());
    }

    Ok(vec![RepairRequest {
        id: get_snow_flake_id_u64(),
        strategy: "binary_search_gap".to_string(),
        gaps,
    }])
}

fn merge_gaps(gaps: Vec<ValidationGap>) -> Vec<ValidationGap> {
    if gaps.is_empty() {
        return Vec::new();
    }

    let mut missing: Vec<(String, String, String, u64, u64)> = Vec::new();
    let mut others: Vec<ValidationGap> = Vec::new();

    for gap in gaps.into_iter() {
        match gap {
            ValidationGap::MissingData {
                symbol,
                trade_type,
                start_time,
                end_time,
                table,
            } => missing.push((table, symbol, trade_type, start_time, end_time)),
            other => others.push(other),
        }
    }

    missing.sort_by(|a, b| a.3.cmp(&b.3));

    let mut merged: Vec<(String, String, String, u64, u64)> = Vec::new();
    for (table, symbol, trade_type, start, end) in missing.into_iter() {
        if let Some(last) = merged.last_mut() {
            if last.0 == table && last.1 == symbol && last.2 == trade_type && start == last.4 {
                if end > last.4 {
                    last.4 = end;
                }
                continue;
            }
        }
        merged.push((table, symbol, trade_type, start, end));
    }

    let mut result = Vec::with_capacity(merged.len() + others.len());
    for (table, symbol, trade_type, start, end) in merged.into_iter() {
        result.push(ValidationGap::MissingData {
            symbol,
            trade_type,
            start_time: start,
            end_time: end,
            table,
        });
    }
    result.extend(others);
    result
}

///
/// 主要用于扇面这个binary_search_gap的方式去提供数据。
/// 因为需要不同的数据数量，所以需要提供一个接口，计算start和end之间的不同数据数量。
///
#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait BinarySearchDataTrait: Send + Sync {
    ///
    /// 计算start和end之间的不同数据数量
    ///
    fn count_distinct_between(&self, start: u64, end: u64) -> Result<u64, YuError>;

    fn table_name(&self) -> String;
}

type BinarySearchDDataSource = Arc<dyn BinarySearchDataTrait>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_integrity::models::ValidationGap;
    use crate::errors::YuError;
    use std::sync::Arc;
    use yue::models::HistoryInterval;

    #[test]
    fn test_binary_search_gap_no_gap() -> Result<(), YuError> {
        let interval = HistoryInterval::FiveMinutes;
        let interval_ms = interval.to_milliseconds();
        let t0: u64 = 1_700_000_000;
        let start = t0;
        let end = t0 + 5 * interval_ms;
        let present: Vec<u64> = (0..5).map(|i| t0 + i * interval_ms).collect();

        // use mockall-generated mock
        let mut mock = MockBinarySearchDataTrait::new();
        mock.expect_table_name().returning(|| "bn_spot_kline".to_string());
        mock.expect_count_distinct_between().returning(move |s: u64, e: u64| {
            let mut c: u64 = 0;
            for &t in present.iter() {
                if t >= s && t < e {
                    c += 1;
                }
            }
            Ok(c)
        });

        let ds = Arc::new(mock);
        let res = binary_search_gap("BTCUSDT", "SPOT", start, end, &interval, ds)?;
        assert!(res.is_empty(), "expected no repair requests but got: {:?}", res);
        Ok(())
    }

    #[test]
    fn test_binary_search_gap_single_missing() -> Result<(), YuError> {
        let interval = HistoryInterval::FiveMinutes;
        let interval_ms = interval.to_milliseconds();
        let t0: u64 = 1_700_000_000;
        let start = t0;
        let end = t0 + 5 * interval_ms;
        let present_vec = vec![t0, t0 + interval_ms, t0 + 3 * interval_ms, t0 + 4 * interval_ms];
        let present = present_vec.clone();

        let mut mock = MockBinarySearchDataTrait::new();
        mock.expect_table_name().returning(|| "bn_spot_kline".to_string());
        mock.expect_count_distinct_between().returning(move |s: u64, e: u64| {
            let mut c: u64 = 0;
            for &t in present.iter() {
                if t >= s && t < e {
                    c += 1;
                }
            }
            Ok(c)
        });

        let ds = Arc::new(mock);
        let reqs = binary_search_gap("BTCUSDT", "SPOT", start, end, &interval, ds)?;
        // Expect at least one RepairRequest containing a MissingData gap for the missing slot
        assert!(!reqs.is_empty(), "expected repair requests but none produced");
        let missing_ts = t0 + 2 * interval_ms;
        let missing_end = missing_ts + interval_ms;
        let mut found = false;
        for req in reqs.iter() {
            for g in req.gaps.iter() {
                if let ValidationGap::MissingData { start_time, end_time, .. } = g {
                    if *start_time == missing_ts && *end_time == missing_end {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "missing slot not reported in repair requests: {:?}", reqs);
        Ok(())
    }

    #[test]
    fn test_binary_search_gap_merge_adjacent() -> Result<(), YuError> {
        let interval = HistoryInterval::FiveMinutes;
        let interval_ms = interval.to_milliseconds();
        let t0: u64 = 1_700_000_000;
        let start = t0;
        let end = t0 + 5 * interval_ms;
        let present = vec![t0, t0 + interval_ms, t0 + 4 * interval_ms];

        let mut mock = MockBinarySearchDataTrait::new();
        mock.expect_table_name().returning(|| "bn_spot_kline".to_string());
        mock.expect_count_distinct_between().returning(move |s: u64, e: u64| {
            let mut c: u64 = 0;
            for &t in present.iter() {
                if t >= s && t < e {
                    c += 1;
                }
            }
            Ok(c)
        });

        let ds = Arc::new(mock);
        let reqs = binary_search_gap("BTCUSDT", "SPOT", start, end, &interval, ds)?;
        assert_eq!(reqs.len(), 1, "adjacent gaps should be merged into one request");
        assert_eq!(reqs[0].gaps.len(), 1, "adjacent gaps should be merged into one gap");

        match &reqs[0].gaps[0] {
            ValidationGap::MissingData { start_time, end_time, .. } => {
                assert_eq!(*start_time, t0 + 2 * interval_ms);
                assert_eq!(*end_time, t0 + 4 * interval_ms);
            }
            other => panic!("unexpected gap variant: {:?}", other),
        }
        Ok(())
    }

    #[test]
    fn test_binary_search_gap_merge_multiple_ranges() -> Result<(), YuError> {
        let interval = HistoryInterval::FiveMinutes;
        let interval_ms = interval.to_milliseconds();
        let t0: u64 = 1_700_000_000;
        let start = t0;
        let end = t0 + 10 * interval_ms;
        let present = vec![t0, t0 + interval_ms, t0 + 4 * interval_ms, t0 + 8 * interval_ms];

        let mut mock = MockBinarySearchDataTrait::new();
        mock.expect_table_name().returning(|| "bn_spot_kline".to_string());
        mock.expect_count_distinct_between().returning(move |s: u64, e: u64| {
            let mut c: u64 = 0;
            for &t in present.iter() {
                if t >= s && t < e {
                    c += 1;
                }
            }
            Ok(c)
        });

        let ds = Arc::new(mock);
        let reqs = binary_search_gap("BTCUSDT", "SPOT", start, end, &interval, ds)?;

        assert_eq!(reqs.len(), 1, "multiple gaps should be returned in one request");
        assert_eq!(reqs[0].gaps.len(), 3, "expected three merged gaps");

        let expected = vec![
            (t0 + 2 * interval_ms, t0 + 4 * interval_ms),
            (t0 + 5 * interval_ms, t0 + 8 * interval_ms),
            (t0 + 9 * interval_ms, t0 + 10 * interval_ms),
        ];

        for (gap, (exp_start, exp_end)) in reqs[0].gaps.iter().zip(expected.into_iter()) {
            match gap {
                ValidationGap::MissingData { start_time, end_time, .. } => {
                    assert_eq!(*start_time, exp_start);
                    assert_eq!(*end_time, exp_end);
                }
                other => panic!("unexpected gap variant: {:?}", other),
            }
        }

        Ok(())
    }
}
