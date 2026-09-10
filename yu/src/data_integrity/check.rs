use crate::data_integrity::models::{ValidationGap, ValidationResult};
use crate::duck_db::DuckDBDSProvider;
use crate::errors::YuError;
use async_trait::async_trait;
use log::warn;
use std::sync::Arc;
use std::time::{Duration, Instant};
use yue::models::HistoryInterval;
use yue::query_message::DataSourceProviderTrait;

/// 可插拔校验策略接口，Checker 调用实现校验逻辑。
#[async_trait]
pub trait ValidationStrategyTrait: Send + Sync {
    async fn validate(&self) -> Result<Option<ValidationResult>, YuError>;

    fn name(&self) -> String;
}

/// # 二分法具体查找方法。
/// 1. 设定start_time = min_timestamp,end_time = max_timestamp
/// 2. 计算mid_time = (start_time + end_time) / 2
/// 3. 设定start_time,计算mid_time计算有多少个interval_seconds的时间段，为time_slots
/// 4，通过sql，判断start_time到mid_time的记录数是否等于time_slots，如果等于，说明左半部分没有缺失数据
/// 5. 用同样办法检查右半
/// 6. 递归执行2-5步，直到找到所有缺失的时间段
/// 7. 找出的缺失时间段。都加入到ValidationResult返回
///
/// # gaps的要求
/// 1. end_time为数据库的close_time+1。
/// 2. 返回的gaps的start_time不允许小于start，end_time不允许大于end。
///
/// 数据说明
/// 1. 数据库中的数据，candle_begin_time和close_time相差的是interval-1。
///    - 比如说candle_begin_time是0， interval是300_000，那么close_time是299_999
/// 2. 传入的数据必须是interval的整点。
///
///
pub fn binary_search_gap(
    identify: &str,
    trade_type: &str,
    start: u64,
    end: u64,
    interval: &HistoryInterval,
    data_source: BinarySearchDS,
) -> Result<Vec<ValidationGap>, YuError> {
    // defensive
    if start >= end {
        return Ok(Vec::new());
    }

    let interval_ms = interval.to_milliseconds();
    let expected = (end - start) / interval_ms;

    // count existing distinct slots in [start, end)
    let actual = data_source.count_distinct_between(identify, start, end)?;
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
        return Ok(vec![gap]);
    }

    // compute aligned mid
    let mid = start + ((end - start) / 2 / interval_ms) * interval_ms;

    // recurse left and right
    let mut left = binary_search_gap(identify, trade_type, start, mid, interval, data_source.clone())?;
    let mut right = binary_search_gap(identify, trade_type, mid, end, interval, data_source.clone())?;
    left.append(&mut right);
    let mut gaps: Vec<ValidationGap> = left;
    gaps.extend(right);
    let gaps = merge_gaps(gaps);
    if gaps.is_empty() {
        return Ok(Vec::new());
    }

    Ok(gaps)
}

///
///  gaps。
///  1. ValidationGap的排序都是按 照其start_time进行排序
///  2. 传入的ValidationGap都是interval_ms的最小单位。就是其start_time和end_time之间的差值，都是一个interval_ms的长度。
///  3. 传入的gaps可能是无序的。
///  4. 所有的gap的symbol和trade_type都是相同的。
///  5. 是否相邻（规则 ：相邻定义为 next.start_time == prev.end_time）
///  6. gap之间不可能有交集。比如[ (100, 200), (150, 300) ] 不可能出现在传入的gaps中。
///
///  gaps的合并
///  gaps是无序的，需要先排序，然后合并相邻的时间段
///  一下是几个案例
///
///  1. gaps = [ (t1, t2), (t2, t3) ] =>>  merged_gaps = [ (t1,  t3) ]
///  2. gaps = [ (t1, t2), (t2, t3), (t5, t6) , (t6, t7) , (t8, t9)] =>>  merged_gaps = [ (t1,  t3)，(t5,  t7)，(t8, t9)]
///  注意：根据注释约定，传入的 gaps 不会相互重叠，合并规则只需要处理“相邻”的情况（端点相等）。
pub(crate) fn merge_gaps(gaps: Vec<ValidationGap>) -> Vec<ValidationGap> {
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
pub trait BinarySearchDSTrait: Send + Sync {
    ///
    /// 计算start和end之间的不同数据数量
    ///
    fn count_distinct_between(&self, identify: &str, start: u64, end: u64) -> Result<u64, YuError>;

    fn table_name(&self) -> String;

    fn symbol_column(&self) -> String;

    fn time_column(&self) -> String;
}

pub type BinarySearchDS = Arc<dyn BinarySearchDSTrait>;

pub struct DuckDBBinarySearchDataImpl {
    db_provider: DuckDBDSProvider, // Database provider for data access
    table_name: String,            // Table to validate
    time_column: String,           // 时间检测列，改列的时间都是unix时间戳，单位毫秒
    symbol_column: String,         // symbol的column
}

impl DuckDBBinarySearchDataImpl {
    pub fn new(db_provider: DuckDBDSProvider, table_name: String, time_column: String, symbol_column: String) -> BinarySearchDS {
        Arc::new(Self {
            db_provider,
            table_name,
            time_column,
            symbol_column,
        })
    }
}

impl BinarySearchDSTrait for DuckDBBinarySearchDataImpl {
    fn count_distinct_between(&self, identify: &str, start: u64, end: u64) -> Result<u64, YuError> {
        let sql = format!(
            "SELECT COUNT(DISTINCT {time_col}) FROM {table} WHERE {time_col} >= ? AND {time_col} < ? AND {symbol_col}='{symbol}'",
            time_col = self.time_column,
            table = self.table_name,
            symbol_col = self.symbol_column,
            symbol = identify,
        );
        let connection = self.db_provider.acquire()?;
        let mut stmt = connection.prepare(&sql)?;

        let timer = Instant::now();
        let mut rows = stmt
            .query([start as i64, end as i64])
            .map_err(|_| YuError::new(&format!("{}, query gap from {} to {}", identify, start, end)))?;
        let elapsed = timer.elapsed();
        if elapsed > Duration::from_secs(10) {
            warn!(
                "Slow SQL (>10s) in count_distinct_between: sql={}, params=[start={}, end={}], elapsed={:?}",
                sql, start, end, elapsed
            );
        }

        if let Some(row) = rows.next()? {
            let c: i64 = row.get(0)?;
            Ok(c as u64)
        } else {
            Ok(0)
        }
    }

    fn table_name(&self) -> String {
        self.table_name.clone()
    }

    fn symbol_column(&self) -> String {
        self.symbol_column.clone()
    }

    fn time_column(&self) -> String {
        self.time_column.clone()
    }
}

///
/// 因为同步客户端的数据库基本都是history表和instruments表分立。
/// 而服务器端来的基本server_id在instruments表中，所以需要join instruments表来计算count。
/// 所以需要
/// 1. history表中有instrument_id列，指向instruments表的id列
/// 2. instruments表中有server_id列，指向服务器端的server_id
///
pub struct SyncClientBinarySearchDataImpl {
    pg_pool: sqlx::PgPool,
    history_table_name: String,
    instrument_table_name: String,
    time_column: String,
    count_sql: &'static str,
}

impl SyncClientBinarySearchDataImpl {
    pub fn new(pg_pool: sqlx::PgPool, table_name: &str, instrument_table_name: &str, time_column: &str) -> BinarySearchDS {
        // construct SQL statements once and leak to &'static str after manual audit
        let count_sql = format!(
            "SELECT COUNT(DISTINCT h.{time_col}) FROM {table} h join {instrument_table_name} i on h.instrument_id = i.id WHERE h.{time_col} >= $1 AND h.{time_col} < $2 AND i.server_id = $3",
            time_col = time_column,
            table = table_name,
            instrument_table_name = instrument_table_name
        );

        let count_sql_static: &'static str = Box::leak(count_sql.into_boxed_str());

        Arc::new(Self {
            pg_pool,
            history_table_name: table_name.to_string(),
            time_column: time_column.to_string(),
            instrument_table_name: instrument_table_name.to_string(),
            count_sql: count_sql_static,
        })
    }

    pub fn instrument_table_name(&self) -> String {
        self.instrument_table_name.clone()
    }
}

impl BinarySearchDSTrait for SyncClientBinarySearchDataImpl {
    fn count_distinct_between(&self, identify: &str, start: u64, end: u64) -> Result<u64, YuError> {
        // use prebuilt, leaked SQL string
        let sql_static = self.count_sql;
        let pool = self.pg_pool.clone();
        // convert milliseconds to chrono::DateTime<Utc> so bindings match timestamptz columns
        let start_dt = {
            let secs = (start / 1000) as i64;
            let nsecs = ((start % 1000) * 1_000_000) as u32;
            chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nsecs)
        };
        let end_dt = {
            let secs = (end / 1000) as i64;
            let nsecs = ((end % 1000) * 1_000_000) as u32;
            chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nsecs)
        };

        // parse identify (server_id) into integer to match bigint column type
        let identify_int = identify
            .parse::<i64>()
            .map_err(|_| YuError::new(&format!("invalid server id: {}", identify)))?;

        let timer = Instant::now();

        // Run the async query in a dedicated thread with its own runtime and send result back via channel
        let (tx, rx) = std::sync::mpsc::channel::<Result<i64, String>>();
        let pool_cloned = pool.clone();
        let sql_static_clone = sql_static;
        let start_dt_clone = start_dt;
        let end_dt_clone = end_dt;
        let identify_int_clone = identify_int;

        std::thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(Err(format!("create runtime error: {}", e)));
                    return;
                }
            };

            let fut_res = rt.block_on(async move {
                sqlx::query_scalar::<_, i64>(sql_static_clone)
                    .bind(start_dt_clone)
                    .bind(end_dt_clone)
                    .bind(identify_int_clone)
                    .fetch_one(&pool_cloned)
                    .await
            });

            let _ = match fut_res {
                Ok(v) => tx.send(Ok(v)),
                Err(e) => tx.send(Err(e.to_string())),
            };
        });

        // wait with timeout
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(c)) => {
                let elapsed = timer.elapsed();
                if elapsed > Duration::from_secs(10) {
                    warn!(
                        "Slow SQL (>10s) in count_distinct_between: sql={}, params=[start={}, end={}, identify={}], elapsed={:?}",
                        sql_static,
                        start_dt.unwrap().to_rfc3339(),
                        end_dt.unwrap().to_rfc3339(),
                        identify_int,
                        elapsed
                    );
                }
                Ok(c as u64)
            }
            Ok(Err(err_str)) => Err(YuError::new(&format!("{}, query gap from {} to {}: {}", identify, start, end, err_str))),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                warn!(
                    "Slow SQL timeout (>=10s) in count_distinct_between: sql={}, params=[start={}, end={}, identify={}], elapsed>=10s",
                    sql_static,
                    start_dt.unwrap().to_rfc3339(),
                    end_dt.unwrap().to_rfc3339(),
                    identify_int
                );
                Err(YuError::new(&format!(
                    "{}, query gap from {} to {}: timeout after 10s",
                    identify, start, end
                )))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(YuError::new(&format!(
                "{}, query gap from {} to {}: executor disconnected",
                identify, start, end
            ))),
        }
    }

    fn table_name(&self) -> String {
        self.history_table_name.clone()
    }

    fn symbol_column(&self) -> String {
        "server_id".to_string() // hardcoded for sync client, as server_id is used to identify the symbol
    }

    fn time_column(&self) -> String {
        self.time_column.clone()
    }
}

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
        let mut mock = MockBinarySearchDSTrait::new();
        mock.expect_table_name().returning(|| "bn_spot_kline".to_string());
        mock.expect_count_distinct_between().returning(move |_: &str, s: u64, e: u64| {
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
    fn test_binary_search_gap_no_gap_even() -> Result<(), YuError> {
        // even number of slots (10)
        let interval = HistoryInterval::FiveMinutes;
        let interval_ms = interval.to_milliseconds();
        let t0: u64 = 1_700_000_000;
        let start = t0;
        // 10 slots: t0, t0+1*interval_ms, ..., t0+9*interval_ms
        let end = t0 + 10 * interval_ms;
        let present: Vec<u64> = (0..10).map(|i| t0 + i * interval_ms).collect();

        let mut mock = MockBinarySearchDSTrait::new();
        mock.expect_table_name().returning(|| "bn_spot_kline".to_string());
        mock.expect_count_distinct_between().returning(move |_: &str, s: u64, e: u64| {
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
        assert!(res.is_empty(), "expected no repair requests for even count but got: {:?}", res);
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

        let mut mock = MockBinarySearchDSTrait::new();
        mock.expect_table_name().returning(|| "bn_spot_kline".to_string());
        mock.expect_count_distinct_between().returning(move |_: &str, s: u64, e: u64| {
            let mut c: u64 = 0;
            for &t in present.iter() {
                if t >= s && t < e {
                    c += 1;
                }
            }
            Ok(c)
        });

        let ds = Arc::new(mock);
        let gaps = binary_search_gap("BTCUSDT", "SPOT", start, end, &interval, ds)?;
        assert!(!gaps.is_empty(), "expected gaps but none produced");
        let missing_ts = t0 + 2 * interval_ms;
        let missing_end = missing_ts + interval_ms;
        let mut found = false;
        for g in gaps.iter() {
            if let ValidationGap::MissingData { start_time, end_time, .. } = g {
                if *start_time == missing_ts && *end_time == missing_end {
                    found = true;
                }
            }
        }
        assert!(found, "missing slot not reported in gaps: {:?}", gaps);
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

        let mut mock = MockBinarySearchDSTrait::new();
        mock.expect_table_name().returning(|| "bn_spot_kline".to_string());
        mock.expect_count_distinct_between().returning(move |_: &str, s: u64, e: u64| {
            let mut c: u64 = 0;
            for &t in present.iter() {
                if t >= s && t < e {
                    c += 1;
                }
            }
            Ok(c)
        });

        let ds = Arc::new(mock);
        let gaps = binary_search_gap("BTCUSDT", "SPOT", start, end, &interval, ds)?;
        assert_eq!(gaps.len(), 1, "adjacent gaps should be merged into one gap");

        match &gaps[0] {
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

        let mut mock = MockBinarySearchDSTrait::new();
        mock.expect_table_name().returning(|| "bn_spot_kline".to_string());
        mock.expect_count_distinct_between().returning(move |_: &str, s: u64, e: u64| {
            let mut c: u64 = 0;
            for &t in present.iter() {
                if t >= s && t < e {
                    c += 1;
                }
            }
            Ok(c)
        });

        let ds = Arc::new(mock);
        let gaps = binary_search_gap("BTCUSDT", "SPOT", start, end, &interval, ds)?;

        assert_eq!(gaps.len(), 3, "expected three merged gaps");

        let expected = vec![
            (t0 + 2 * interval_ms, t0 + 4 * interval_ms),
            (t0 + 5 * interval_ms, t0 + 8 * interval_ms),
            (t0 + 9 * interval_ms, t0 + 10 * interval_ms),
        ];

        for (gap, (exp_start, exp_end)) in gaps.iter().zip(expected.into_iter()) {
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

    #[test]
    fn test_merge_gaps_adjacent() {
        let gaps = vec![
            ValidationGap::MissingData {
                symbol: "BTCUSDT".to_string(),
                trade_type: "SPOT".to_string(),
                start_time: 100,
                end_time: 200,
                table: "t1".to_string(),
            },
            ValidationGap::MissingData {
                symbol: "BTCUSDT".to_string(),
                trade_type: "SPOT".to_string(),
                start_time: 200,
                end_time: 300,
                table: "t1".to_string(),
            },
            ValidationGap::MissingData {
                symbol: "BTCUSDT".to_string(),
                trade_type: "SPOT".to_string(),
                start_time: 300,
                end_time: 400,
                table: "t1".to_string(),
            },
        ];

        let merged = merge_gaps(gaps);
        assert_eq!(merged.len(), 1, "expected single merged gap, got: {:?}", merged);
        match &merged[0] {
            ValidationGap::MissingData {
                start_time,
                end_time,
                symbol,
                table,
                ..
            } => {
                assert_eq!(*start_time, 100);
                assert_eq!(*end_time, 400);
                assert_eq!(symbol, "BTCUSDT");
                assert_eq!(table, "t1");
            }
            _ => panic!("unexpected gap variant"),
        }
    }

    #[test]
    fn test_merge_multi_gaps() {
        let gaps = vec![
            ValidationGap::MissingData {
                symbol: "BTCUSDT".to_string(),
                trade_type: "SPOT".to_string(),
                start_time: 100,
                end_time: 200,
                table: "t1".to_string(),
            },
            ValidationGap::MissingData {
                symbol: "BTCUSDT".to_string(),
                trade_type: "SPOT".to_string(),
                start_time: 200,
                end_time: 300,
                table: "t1".to_string(),
            },
            ValidationGap::MissingData {
                symbol: "BTCUSDT".to_string(),
                trade_type: "SPOT".to_string(),
                start_time: 300,
                end_time: 400,
                table: "t1".to_string(),
            },
            ValidationGap::MissingData {
                symbol: "BTCUSDT".to_string(),
                trade_type: "SPOT".to_string(),
                start_time: 500,
                end_time: 600,
                table: "t1".to_string(),
            },
            ValidationGap::MissingData {
                symbol: "BTCUSDT".to_string(),
                trade_type: "SPOT".to_string(),
                start_time: 600,
                end_time: 700,
                table: "t1".to_string(),
            },
            ValidationGap::MissingData {
                symbol: "BTCUSDT".to_string(),
                trade_type: "SPOT".to_string(),
                start_time: 800,
                end_time: 900,
                table: "t1".to_string(),
            },
        ];

        let merged = merge_gaps(gaps);
        assert_eq!(merged.len(), 3, "expected three merged gaps, got: {:?}", merged);

        let expected: Vec<(u64, u64)> = vec![(100, 400), (500, 700), (800, 900)];
        for (i, gap) in merged.iter().enumerate() {
            match gap {
                ValidationGap::MissingData {
                    start_time,
                    end_time,
                    symbol,
                    table,
                    ..
                } => {
                    assert_eq!(*start_time, expected[i].0);
                    assert_eq!(*end_time, expected[i].1);
                    assert_eq!(symbol, "BTCUSDT");
                    assert_eq!(table, "t1");
                }
                _ => panic!("unexpected gap variant"),
            }
        }
    }

    #[test]
    fn test_merge_gaps_single_and_empty() {
        let single = vec![ValidationGap::MissingData {
            symbol: "BTCUSDT".to_string(),
            trade_type: "SPOT".to_string(),
            start_time: 777,
            end_time: 888,
            table: "t1".to_string(),
        }];
        let merged_single = merge_gaps(single);
        assert_eq!(merged_single.len(), 1, "single gap should remain single");
        if let ValidationGap::MissingData { start_time, end_time, .. } = &merged_single[0] {
            assert_eq!(*start_time, 777);
            assert_eq!(*end_time, 888);
        }

        let empty: Vec<ValidationGap> = Vec::new();
        let merged_empty = merge_gaps(empty);
        assert!(merged_empty.is_empty(), "empty input should produce empty output");
    }
}
