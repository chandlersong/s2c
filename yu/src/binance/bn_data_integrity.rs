use crate::binance::binance_db_consts::BinanceTables;
use crate::data_integrity::check::ValidationStrategy;
use crate::data_integrity::models::{ValidationGap, ValidationResult};
use crate::duck_db::DBProvider;
use crate::errors::YuError;
use async_trait::async_trait;
use log::{debug, error, info};

pub const BN_SPOT_KLINE_CHECK: &str = "binance_spot_check"; // WireMock server address

///
/// 检测 Binance 现货数据完整性的策略实现
/// 以后想要转换成一个通用类。
///
///
#[derive(Clone)]
pub struct SpotCheckStrategy {
    db_provider: DBProvider, // Database provider for data access
    table_name: String,      // Table to validate
    time_column: String,     // 时间检测列，改列的时间都是unix时间戳，单位毫秒
    symbol_column: String,   // symbol的column
    interval_ms: u64,        // Interval in seconds for each validation chunk
    max_allow_gap: u64,      // 当gap超过这点时间，就不算missing。主要是防止下假币反复查询。
}

///
/// 此检测，检测数据库里面存在的数据，而不通过API和服务器通信。如果需要可以以后加入，
///
/// 现阶段，默认认为数据库里面的symbol完整。进行检测。基本原则
///
/// 1. 如果存在symbol和数据，那么数据存在。就进行时间完整性检测。
/// 2. 如果symbol不存在，那么不进行检测。
///
///
/// 等待加入检测
/// 1. symbol是否完整。这个是另外建立一个检测策略，还是其他就另说。
///
impl SpotCheckStrategy {
    pub fn spot_check_strategy(db_source: Option<DBProvider>) -> Self {
        let db_provider = db_source.unwrap_or_else(|| DBProvider::default());
        Self {
            db_provider,
            table_name: BinanceTables::SpotKline.table_name(),
            time_column: "candle_begin_time".to_string(),
            symbol_column: "symbol".to_string(),
            interval_ms: 5 * 60 * 1000,         // 5分钟
            max_allow_gap: 24 * 60 * 60 * 1000, //一天
        }
    }

    // helper: count distinct time rows between [start, end]
    fn count_distinct_between(conn: &mut duckdb::Connection, table: &str, time_col: &str, start: u64, end: u64) -> Result<u64, YuError> {
        let sql = format!(
            "SELECT COUNT(DISTINCT {time_col}) FROM {table} WHERE {time_col} >= ? AND {time_col} <= ?",
            time_col = time_col,
            table = table
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query([start as i64, end as i64])?;
        if let Some(row) = rows.next()? {
            let c: i64 = row.get(0)?;
            Ok(c as u64)
        } else {
            Ok(0)
        }
    }

    /// # 二分法具体查找方法。
    /// 1. 设定start_time = min_timestamp,end_time = max_timestamp
    /// 2. 计算mid_time = (start_time + end_time) / 2
    /// 3. 设定start_time,计算mid_time计算有多少个interval_seconds的时间段，为time_slots
    /// 4，通过sql，判断start_time到mid_time的记录数是否等于time_slots，如果等于，说明左半部分没有缺失数据
    /// 5. 用同样办法检查右半
    /// 6. 递归执行2-5步，直到找到所有缺失的时间段
    /// 7. 找出的缺失时间段。都加入到ValidationResult返回
    fn find_gaps_rec(
        conn: &mut duckdb::Connection,
        table: &str,
        time_col: &str,
        interval_ms: u64,
        start: u64,
        end: u64,
        gaps: &mut Vec<ValidationGap>,
    ) -> Result<(), YuError> {
        if start > end {
            return Ok(());
        }
        let expected = (end - start) / interval_ms + 1;
        let actual = Self::count_distinct_between(conn, table, time_col, start, end)?;
        if actual == expected {
            return Ok(());
        }
        // if the window is a single slot, it's missing
        if start + interval_ms >= end {
            // record a missing gap
            gaps.push(ValidationGap::MissingData {
                symbol: "*".to_string(),
                trade_type: "SPOT".to_string(),
                start_time: start,
                end_time: end,
                table: table.to_string(),
            });
            return Ok(());
        }
        let mid = start + ((end - start) / 2 / interval_ms) * interval_ms; // align mid to interval boundary
                                                                           // ensure mid >= start
        let mid = if mid <= start { start + interval_ms } else { mid };

        // left
        Self::find_gaps_rec(conn, table, time_col, interval_ms, start, mid - interval_ms, gaps)?;
        // right
        Self::find_gaps_rec(conn, table, time_col, interval_ms, mid, end, gaps)?;
        Ok(())
    }

    ///
    ///  gaps。
    ///  1. ValidationGap的排序都是按照其start_time进行排序
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
    fn merge_gaps(gaps: &mut Vec<ValidationGap>) -> Vec<ValidationGap> {
        // extract MissingData entries and keep other variants as-is
        if gaps.is_empty() {
            return Vec::new();
        }

        // collect missing data entries into a vector of tuples for sorting
        let mut items: Vec<(String, String, String, u64, u64)> = Vec::new();
        for g in gaps.iter().cloned() {
            match g {
                ValidationGap::MissingData {
                    symbol,
                    trade_type,
                    start_time,
                    end_time,
                    table,
                } => {
                    items.push((table, symbol, trade_type, start_time, end_time));
                }
                _ => {
                    error!("merge_gaps encountered unsupported gap variant: {:?}", g);
                }
            }
        }

        // sort by start_time
        items.sort_by(|a, b| a.3.cmp(&b.3));

        // merge adjacent intervals for same table/symbol/trade_type
        let mut merged_items: Vec<(String, String, String, u64, u64)> = Vec::new();
        for (table, symbol, trade_type, start, end) in items.into_iter() {
            if let Some(last) = merged_items.last_mut() {
                // last: (table, symbol, trade_type, start, end)
                if last.0 == table && last.1 == symbol && last.2 == trade_type {
                    // 简化规则：输入不会重叠，只有当 start == last.end_time 才被视为相邻并合并
                    if start == last.4 {
                        // extend
                        if end > last.4 {
                            last.4 = end;
                        }
                        continue;
                    }
                }
            }
            merged_items.push((table, symbol, trade_type, start, end));
        }

        // rebuild ValidationGap list from merged items
        let mut result: Vec<ValidationGap> = Vec::new();
        for (table, symbol, trade_type, start, end) in merged_items.into_iter() {
            result.push(ValidationGap::MissingData {
                symbol,
                trade_type,
                start_time: start,
                end_time: end,
                table,
            });
        }

        result
    }

    ///
    /// # 检查单个symbol的流程
    /// 1. 从数据库中获取时间列的最小值和最大值，min_timestamp,max_timestamp
    /// 2. 检测max_timestamp，是否和现在时间点相差是否小于interval_seconds，如果小于，直接到第5步。否则执行第三步
    /// 3. 检查max_timestamp和现在时间的差是否大于max_gap_ms，如果大于，则返回空，否则执行第4步
    /// 4. 取最近的整点unix time，然后不断加上interval_seconds，取最大的小于当前时间的为max_timestamp
    /// 5. 计算min_timestamp,max_timestamp有多少个interval_seconds的时间段，为time_slots
    /// 6. 通过sql，判断是否记录数是否等于time_slots，如果等于，说明没有缺失数据，返回Ok(None)
    /// 7. 如果不等于，说明有缺失数据，通过sql，通过二分法开始查找缺失的时间段。
    ///
    fn check_one_symbol(&self, symbol: &str) -> Result<Vec<ValidationGap>, YuError> {
        debug!("check_one_symbol symbol: {} at {}", symbol, self.time_column);
        let mut conn = self.db_provider.acquire()?;
        let sql = format!(
            "SELECT MIN({}), MAX({}) FROM {} Where {} = {}",
            self.time_column, self.time_column, self.table_name, self.symbol_column, symbol
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        let (min_ts_opt, max_ts_opt) = if let Some(row) = rows.next()? {
            let min_v: Option<i64> = row.get(0).ok();
            let max_v: Option<i64> = row.get(1).ok();
            (min_v.map(|v| v as u64), max_v.map(|v| v as u64))
        } else {
            (None, None)
        };

        if min_ts_opt.is_none() || max_ts_opt.is_none() {
            return Ok(vec![]);
        }
        let min_ts = min_ts_opt.unwrap();
        let mut max_ts = max_ts_opt.unwrap();

        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        if (now_ms - max_ts) > self.max_allow_gap {
            // 超过最大gap时间，不进行检测，直接返回空
            info!(
                "symbol:{} 可能已经下架，不再检测，now_ms:{}, max_ts:{},max allow gap:{}",
                symbol, now_ms, max_ts, self.max_allow_gap
            );
            return Ok(vec![]);
        }

        if now_ms > max_ts {
            let delta = now_ms - max_ts;
            if delta > self.interval_ms {
                // align to interval boundary from earliest point: align max_ts to the latest slot before now
                let remainder = max_ts % self.interval_ms;
                max_ts = max_ts - remainder;
                // DO NOT advance max_ts toward now_ms to avoid creating a huge search window which would
                // dramatically increase the number of expected slots and make binary search impractical.
            }
        }

        if max_ts < min_ts {
            return Ok(vec![]);
        }

        let expected_slots = (max_ts - min_ts) / self.interval_ms + 1;
        let actual = Self::count_distinct_between(&mut conn, &self.table_name, &self.time_column, min_ts, max_ts)?;
        if actual == expected_slots {
            return Ok(vec![]);
        }

        let mut gaps: Vec<ValidationGap> = Vec::new();
        Self::find_gaps_rec(
            &mut conn,
            &self.table_name,
            &self.time_column,
            self.interval_ms,
            min_ts,
            max_ts,
            &mut gaps,
        )?;

        // 合并相邻的缺失区间以便返回更简洁的结果（假设输入 gaps 无重叠）
        let merged_gaps = Self::merge_gaps(&mut gaps);

        if merged_gaps.is_empty() {
            return Ok(vec![]);
        }
        Ok(merged_gaps)
    }
}

#[async_trait]
impl ValidationStrategy for SpotCheckStrategy {
    ///
    ///
    /// 这个主要检测出，时间列是否连续，有没有缺失的时间段。
    ///
    /// 1. 查询当前数据库。把所有的symbol查询出来
    /// 2. 多线程调用check_one_symbol检测所有的数据。
    /// 3， 汇总所有的ValidationGap，如果就返回，没有返回None
    ///
    async fn validate(&self) -> Result<Option<ValidationResult>, YuError> {
        // 1. 查询所有 distinct symbol
        let symbols: Vec<String> = {
            let conn = self.db_provider.acquire()?;
            let sql = format!(
                "SELECT DISTINCT {sym} FROM {table} WHERE {sym} IS NOT NULL",
                sym = self.symbol_column,
                table = self.table_name
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query([])?;
            let mut symbols: Vec<String> = Vec::new();
            while let Some(row) = rows.next()? {
                // try to read as String
                let v: Option<String> = row.get(0).ok();
                if let Some(s) = v {
                    symbols.push(s);
                }
            }
            symbols
        };

        if symbols.is_empty() {
            // no symbols => nothing to validate
            return Ok(None);
        }

        // 2. 并行检查每个 symbol（check_one_symbol 是同步 DB 操作，使用 spawn_blocking）
        let mut handles = Vec::new();
        for sym in symbols.into_iter() {
            let strategy = self.clone();
            let s = sym.clone();
            let h = tokio::task::spawn_blocking(move || strategy.check_one_symbol(&s));
            handles.push(h);
        }

        // 3. 收集所有 gaps
        let mut all_gaps: Vec<ValidationGap> = Vec::new();
        for h in handles {
            match h.await {
                Ok(Ok(mut gaps)) => {
                    all_gaps.append(&mut gaps);
                }
                Ok(Err(e)) => {
                    error!("check_one_symbol returned error: {:?}", e);
                    return Err(e);
                }
                Err(e) => {
                    error!("spawn_blocking join error: {:?}", e);
                    // continue on join error
                }
            }
        }

        if all_gaps.is_empty() {
            return Ok(None);
        }

        // 合并并返回 ValidationResult
        let merged = Self::merge_gaps(&mut all_gaps);
        let result = ValidationResult {
            id: yue::tools::get_snow_flake_id_u64(),
            strategy: self.name().to_string(),
            gaps: merged,
            retry_count: 0,
            error: None,
        };

        Ok(Some(result))
    }

    fn name(&self) -> &'static str {
        BN_SPOT_KLINE_CHECK
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binance::binance_db_consts::CREATE_SPOT_KLINE_TABLE;
    use crate::data_integrity::models::ValidationGap;
    use crate::errors::YuError;
    use crate::test_utils::create_memory_db_provider;

    /// 测试说明（无缺失场景）:
    /// 场景: 在表中连续插入 5 个按 interval 对齐的时间槽数据，范围为 [start, end]
    /// 输入: 连续的 candle_begin_time（没有缺失）
    /// 预期: 调用 find_gaps_rec 后 gaps 为空（没有发现缺失区间）
    #[actix_rt::test]
    async fn test_find_gaps_rec_no_gap() -> Result<(), YuError> {
        let db = create_memory_db_provider();
        let mut conn = db.acquire()?;
        conn.execute_batch(CREATE_SPOT_KLINE_TABLE)?;
        let t0: i64 = 1_700_000_000;
        let interval = 5 * 60 * 1000;
        for i in 0..5 {
            let ts = t0 + i * interval;
            let sql = format!("INSERT INTO bn_spot_kline (id, symbol, candle_begin_time, open, high, low, close, volume, quote_volume, number_of_trades, taker_buy_base_asset_volume, taker_buy_quote_asset_volume, close_time, interval, first_trade_id, last_trade_id) VALUES ({}, 'BTCUSDT', {}, 0,0,0,0,0,0,0,0,0,{}, 1, 0, 0);", i, ts, ts+interval);
            conn.execute_batch(&sql)?;
        }

        let start = t0 as u64;
        let end = (t0 + (5 - 1) * interval) as u64;
        let mut gaps: Vec<ValidationGap> = Vec::new();
        SpotCheckStrategy::find_gaps_rec(&mut conn, "bn_spot_kline", "candle_begin_time", interval as u64, start, end, &mut gaps)?;
        assert!(gaps.is_empty(), "expected no gaps but found: {:?}", gaps);
        Ok(())
    }

    /// 测试说明（单个缺失槽）:
    /// 场景: 在一段连续时间序列中刻意跳过中间一个时间槽（slot），其他槽均插入
    /// 输入: 插入 0,1,3,4 四个槽的数据，缺少第 2 个槽
    /// 预期: find_gaps_rec 能检测到至少一个 MissingData gap 覆盖缺失槽的时间点
    #[actix_rt::test]
    async fn test_find_gaps_rec_missing_gap() -> Result<(), YuError> {
        let db = create_memory_db_provider();
        let mut conn = db.acquire()?;
        conn.execute_batch(CREATE_SPOT_KLINE_TABLE)?;
        let t0: i64 = 1_700_000_000;
        let interval = 5 * 60 * 1000;
        for i in 0..5 {
            if i == 2 {
                continue;
            }
            let ts = t0 + i * interval;
            let sql = format!("INSERT INTO bn_spot_kline (id, symbol, candle_begin_time, open, high, low, close, volume, quote_volume, number_of_trades, taker_buy_base_asset_volume, taker_buy_quote_asset_volume, close_time, interval, first_trade_id, last_trade_id) VALUES ({}, 'BTCUSDT', {}, 0,0,0,0,0,0,0,0,0,{}, 1, 0, 0);", i, ts, ts+interval);
            conn.execute_batch(&sql)?;
        }

        let start = t0 as u64;
        let end = (t0 + (5 - 1) * interval) as u64;
        let mut gaps: Vec<ValidationGap> = Vec::new();
        SpotCheckStrategy::find_gaps_rec(&mut conn, "bn_spot_kline", "candle_begin_time", interval as u64, start, end, &mut gaps)?;
        assert!(!gaps.is_empty(), "expected gaps but found none");
        // ensure one gap covers the missing slot at t0 + 2*interval
        let missing_ts = (t0 + 2 * interval) as u64;
        let mut found = false;
        for g in gaps.iter() {
            if let ValidationGap::MissingData { start_time, end_time, .. } = g {
                if *start_time <= missing_ts && *end_time >= missing_ts {
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "missing slot not detected in gaps: {:?}", gaps);
        Ok(())
    }

    /// 测试说明（合并相邻与重叠）:
    /// 场景: 构造一组缺失区间，其中包含端点相等（相邻）与重叠的区间
    /// 输入: gaps = [(100,200),(200,300),(300,400)] （同 table/symbol/trade_type）
    /// 预期: 合并后变为单个区间 (100,400)
    /// 边界: 验证端点相等会被视作相邻并合并
    #[test]
    fn test_merge_gaps_adjacent() {
        let mut gaps: Vec<ValidationGap> = Vec::new();
        gaps.push(ValidationGap::MissingData {
            symbol: "BTCUSDT".to_string(),
            trade_type: "SPOT".to_string(),
            start_time: 100,
            end_time: 200,
            table: "t1".to_string(),
        });
        // adjacent (touching) - start == previous.end_time
        gaps.push(ValidationGap::MissingData {
            symbol: "BTCUSDT".to_string(),
            trade_type: "SPOT".to_string(),
            start_time: 200,
            end_time: 300,
            table: "t1".to_string(),
        });
        // overlapping with previous merged range
        gaps.push(ValidationGap::MissingData {
            symbol: "BTCUSDT".to_string(),
            trade_type: "SPOT".to_string(),
            start_time: 300,
            end_time: 400,
            table: "t1".to_string(),
        });

        let merged = SpotCheckStrategy::merge_gaps(&mut gaps);
        // 期望：单个合并区间覆盖从最小 start 到最大 end
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

    /// 测试说明（合并相邻与重叠）:
    /// 场景: 构造一组缺失区间，其中包含端点相等（相邻）与重叠的区间
    /// 输入: gaps = [(100,200),(200,300),(500,600),(600,700),(800,900)] （同 table/symbol/trade_type）
    /// 预期: 合并后变为单个区间 [(100,300),(500,700),(800,900)]
    /// 边界: 验证端点相等会被视作相邻并合并
    #[test]
    fn test_merge_multi_gaps() {
        let mut gaps: Vec<ValidationGap> = Vec::new();
        gaps.push(ValidationGap::MissingData {
            symbol: "BTCUSDT".to_string(),
            trade_type: "SPOT".to_string(),
            start_time: 100,
            end_time: 200,
            table: "t1".to_string(),
        });
        // adjacent (touching) - start == previous.end_time
        gaps.push(ValidationGap::MissingData {
            symbol: "BTCUSDT".to_string(),
            trade_type: "SPOT".to_string(),
            start_time: 200,
            end_time: 300,
            table: "t1".to_string(),
        });
        // overlapping with previous merged range
        gaps.push(ValidationGap::MissingData {
            symbol: "BTCUSDT".to_string(),
            trade_type: "SPOT".to_string(),
            start_time: 300,
            end_time: 400,
            table: "t1".to_string(),
        });
        // separate range
        gaps.push(ValidationGap::MissingData {
            symbol: "BTCUSDT".to_string(),
            trade_type: "SPOT".to_string(),
            start_time: 500,
            end_time: 600,
            table: "t1".to_string(),
        });
        // adjacent to previous
        gaps.push(ValidationGap::MissingData {
            symbol: "BTCUSDT".to_string(),
            trade_type: "SPOT".to_string(),
            start_time: 600,
            end_time: 700,
            table: "t1".to_string(),
        });
        // separate range
        gaps.push(ValidationGap::MissingData {
            symbol: "BTCUSDT".to_string(),
            trade_type: "SPOT".to_string(),
            start_time: 800,
            end_time: 900,
            table: "t1".to_string(),
        });

        let merged = SpotCheckStrategy::merge_gaps(&mut gaps);
        // 期望：合并后有三个区间，分别是 [100, 400], [500, 700], [800, 900]
        assert_eq!(merged.len(), 3, "expected three merged gaps, got: {:?}", merged);

        // 验证每个合并后的区间
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

    /// 测试说明（单个 gap 与空输入）:
    /// 场景1: 传入单个缺失区间
    /// 输入1: gaps = [(777,888)]
    /// 预期1: 返回包含同一单区间
    /// 场景2: 传入空列表
    /// 输入2: []
    /// 预期2: 返回空
    #[test]
    fn test_merge_gaps_single_and_empty() {
        // 单个 gap
        let mut single: Vec<ValidationGap> = Vec::new();
        single.push(ValidationGap::MissingData {
            symbol: "BTCUSDT".to_string(),
            trade_type: "SPOT".to_string(),
            start_time: 777,
            end_time: 888,
            table: "t1".to_string(),
        });
        let merged_single = SpotCheckStrategy::merge_gaps(&mut single);
        assert_eq!(merged_single.len(), 1, "single gap should remain single");
        if let ValidationGap::MissingData { start_time, end_time, .. } = &merged_single[0] {
            assert_eq!(*start_time, 777);
            assert_eq!(*end_time, 888);
        }

        // 空输入
        let mut empty: Vec<ValidationGap> = Vec::new();
        let merged_empty = SpotCheckStrategy::merge_gaps(&mut empty);
        assert!(merged_empty.is_empty(), "empty input should produce empty output");
    }
}
