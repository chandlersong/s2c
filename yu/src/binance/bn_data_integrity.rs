use crate::binance::bn_backend_service::{get_spot_kline_table, get_swap_kline_table};
use crate::binance::db_consts::BinanceTables;
use crate::binance::history::HistoryKlineSaver;
use crate::data_integrity::check::ValidationStrategyTrait;
use crate::data_integrity::models::{RepairRequest, ValidationGap, ValidationResult};
use crate::data_integrity::repair::RepairStrategyTrait;
use crate::duck_db::DBProvider;
use crate::duck_db_tables::DuckDbTableTrait;
use crate::errors::YuError;
use async_trait::async_trait;
use governor::Jitter;
use li::tools::time::unix_2_readable;
use log::{debug, error, info, trace, Level};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, Semaphore};
use yue::binance::bn_models::common::SymbolType;
use yue::binance::bn_restful_commands::{SPOT_KLINE_HISTORY_COMMAND, SWAP_KLINE_HISTORY_COMMAND};
use yue::binance::restful_func::{CommonRequestBuilder, HistoryFetcherImpl, HistoryFetcherTrait};
use yue::errors::YueError;
use yue::models::HistoryInterval;

pub const BN_SPOT_KLINE_CHECK: &str = "binance_spot_check"; // WireMock server address
pub const BN_SWAP_KLINE_CHECK: &str = "binance_swap_check"; // WireMock server address

///
/// 判断重复的过程是这样的。
/// 1. 每个symbol返回的gaps，取start的和作为key。因为end_time会变化。判断次数
/// 2. 如果超过次数，则加入ignore列表
///
#[derive(Clone)]
struct IgnoreSymbols {
    symbols: HashSet<String>,
    missing_counts: HashMap<String, u64>,
    max_count: u64,
}

impl IgnoreSymbols {
    pub fn reset(&mut self) {
        self.missing_counts.clear();
    }

    pub fn is_ignored(&self, symbol: &str) -> bool {
        self.symbols.contains(symbol)
    }

    pub fn plus_missing(&mut self, gaps: &Vec<ValidationGap>) -> bool {
        let symbol = match gaps.first() {
            Some(ValidationGap::MissingData { symbol, .. }) => symbol.clone(),
            _ => return true,
        };
        let mut key = 0;
        for gap in gaps {
            match gap {
                ValidationGap::MissingData { start_time, .. } => {
                    key = key + start_time;
                }
                _ => {}
            }
        }
        let real_key = format!("symbol:{}_key:{}", symbol, key);

        let missing_count = self.missing_counts.get(&real_key).unwrap_or(&0) + 1;
        if missing_count > self.max_count {
            info!("symbol:{} 加入更新ignore列表，因为缺失次数超过{}", symbol, self.max_count);
            self.symbols.insert(symbol);
            return false;
        }
        self.missing_counts.insert(real_key, missing_count);
        true
    }
}

impl Default for IgnoreSymbols {
    fn default() -> Self {
        Self {
            symbols: Default::default(),
            missing_counts: Default::default(),
            max_count: 10,
        }
    }
}

///
/// 检测 Binance 现货数据完整性的策略实现
/// 以后想要转换成一个通用类。
///
/// 如果超过3次没有记录，则这个symbol不会再进入再进入check
///
#[derive(Clone)]
pub struct SpotCheckStrategy {
    db_provider: DBProvider,                    // Database provider for data access
    table_name: String,                         // Table to validate
    time_column: String,                        // 时间检测列，改列的时间都是unix时间戳，单位毫秒
    symbol_column: String,                      // symbol的column
    interval: HistoryInterval,                  // Interval in seconds for each validation chunk
    ignore_symbols: Arc<RwLock<IgnoreSymbols>>, // 当gap超过这点时间，就不算missing。主要是防止下假币反复查询。
    data_retention_time: u64,                   // 数据保留时间，超过这个时间的数据，不进行检测
    name: String,
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
    pub fn spot_check_strategy(db_source: Option<DBProvider>, data_retention_time: u64) -> Self {
        let db_provider = db_source.unwrap_or_else(|| DBProvider::default());
        Self {
            db_provider,
            table_name: BinanceTables::SpotKline.table_name(),
            time_column: "candle_begin_time".to_string(),
            symbol_column: "symbol".to_string(),
            interval: HistoryInterval::FiveMinutes, // 5分钟
            ignore_symbols: Arc::new(RwLock::new(IgnoreSymbols::default())),
            data_retention_time,
            name: BN_SPOT_KLINE_CHECK.to_string(),
        }
    }

    pub fn swap_check_strategy(db_source: Option<DBProvider>, data_retention_time: u64) -> Self {
        let db_provider = db_source.unwrap_or_else(|| DBProvider::default());
        Self {
            db_provider,
            table_name: BinanceTables::SwapKline.table_name(),
            time_column: "candle_begin_time".to_string(),
            symbol_column: "symbol".to_string(),
            interval: HistoryInterval::FiveMinutes, // 5分钟
            ignore_symbols: Arc::new(RwLock::new(IgnoreSymbols::default())),
            data_retention_time,
            name: BN_SWAP_KLINE_CHECK.to_string(),
        }
    }

    // helper: count distinct time rows between [start, end]
    fn count_distinct_between(
        &self,
        conn: &mut duckdb::Connection,
        table: &str,
        symbol: &str,
        time_col: &str,
        start: u64,
        end: u64,
    ) -> Result<u64, YuError> {
        let sql = format!(
            "SELECT COUNT(DISTINCT {time_col}) FROM {table} WHERE {time_col} >= ? AND {time_col} < ? AND {symbol_col}='{symbol}'",
            time_col = time_col,
            table = table,
            symbol_col = self.symbol_column,
            symbol = symbol
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt
            .query([start as i64, end as i64])
            .map_err(|_| YuError::new(&format!("{}, query gap from {} to {}", symbol, start, end)))?;
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
    fn find_gaps_rec(
        &self,
        symbol: &str,
        conn: &mut duckdb::Connection,
        table: &str,
        time_col: &str,
        interval: &HistoryInterval,
        start: u64,
        end: u64,
        gaps: &mut Vec<ValidationGap>,
    ) -> Result<(), YuError> {
        let interval_ms = interval.to_milliseconds();
        if start >= end {
            return Ok(());
        }
        let expected = (end - start) / interval_ms;
        let actual = self.count_distinct_between(conn, table, symbol, time_col, start, end)?;
        if actual == expected {
            return Ok(());
        }
        // 基准情况：如果期望的 slot 数为 1，则该窗口只包含单个时间槽，且已经排除了 actual==expected 的情况，说明该槽缺失
        if expected == 1 {
            // 记录缺失区间：end_time 使用修正后的 gap_end
            gaps.push(ValidationGap::MissingData {
                symbol: symbol.to_string(),
                trade_type: "SPOT".to_string(),
                start_time: start,
                end_time: start + interval_ms,
                table: table.to_string(),
            });
            return Ok(());
        }
        let mid = start + ((end - start) / 2 / interval_ms) * interval_ms; // align mid to interval boundary

        // left
        self.find_gaps_rec(symbol, conn, table, time_col, interval, start, mid, gaps)?;
        // right
        self.find_gaps_rec(symbol, conn, table, time_col, interval, mid, end, gaps)?;
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
        //因为取binance查询的时候，包含临界值的话，会把这个临界值为candle_begin_time查询下一个周期。所以这里-1，避免这种问题
        for (table, symbol, trade_type, start, end) in merged_items.into_iter() {
            result.push(ValidationGap::MissingData {
                symbol,
                trade_type,
                start_time: start,
                end_time: end - 1,
                table,
            });
        }

        result
    }

    ///
    /// # 检查单个symbol的流程
    /// 1. 从数据库中获取时间列的最小值和最大值，min_timestamp_db,max_timestamp_db
    /// 2. 检查max_timestamp_db和max_timestamp的差是否大于max_allow_gap，如果大于，则返回空，否则继续执行
    /// 3. 在(max_timestamp-data_retention_time)和min_timestamp_db去最大值为min_timestamp
    /// 4. 计算min_timestamp,max_timestamp有多少个interval_seconds的时间段，为time_slots
    /// 5. 通过sql，判断是否记录数是否等于time_slots，如果等于，说明没有缺失数据，返回Ok(None)
    /// 6. 如果不等于，说明有缺失数据，通过sql，通过二分法开始查找缺失的时间段。
    ///
    pub fn check_one_symbol(&self, symbol: &str, max_timestamp: u64) -> Result<Vec<ValidationGap>, YuError> {
        trace!("check_one_symbol symbol: {} at {}", symbol, self.time_column);
        let mut conn = self
            .db_provider
            .acquire()
            .map_err(|e| YuError::new(&format!("when check {}, query db provider,e is {}", symbol, e)))?;
        let interval_ms = self.interval.to_milliseconds();

        // 使用参数化查询以避免注入，并安全获取 min/max
        let sql = format!(
            "SELECT MIN({}), MAX({}) FROM {} WHERE {} = ?",
            self.time_column, self.time_column, self.table_name, self.symbol_column
        );
        let mut stmt = conn.prepare(&sql).map_err(|_| YuError::new(&format!("{} error at draw sql", symbol)))?;
        let mut rows = stmt
            .query([symbol])
            .map_err(|_| YuError::new(&format!("{}, query min/max timestamp", symbol)))?;
        let timestamp_result = rows.next().unwrap_or_else(|e| {
            error!("error at query max/min timestamp:error is {}", e);
            None
        });
        let (min_ts_opt, max_ts_opt) = if let Some(row) = timestamp_result {
            let min_v: Option<i64> = row.get(0).ok();
            let max_v: Option<i64> = row.get(1).ok();
            (min_v.map(|v| v as u64), max_v.map(|v| v as u64))
        } else {
            (None, None)
        };

        if min_ts_opt.is_none() || max_ts_opt.is_none() {
            return Ok(vec![]);
        }
        let min_ts_db = min_ts_opt.unwrap();

        // 根据 data_retention_time 限制最小时间：取 DB min 和 (provided_max_timestamp - retention) 的较大者
        let retention_floor = max_timestamp.saturating_sub(self.data_retention_time);
        let min_timestamp = std::cmp::max(min_ts_db, retention_floor);

        if max_timestamp <= min_timestamp {
            //AI生成的，健壮编程
            return Ok(vec![]);
        }
        //
        // count_distinct_between查询的时候，max_timestamp是不包含的。而传进来的max_timestamp可能不是整点
        // 可能大1
        // 如果开始是00分， max_timestamp是36
        // 那么sql查询就会是 8个slot。但是计算下来是7个，那么后面计算find_gaps_rec就不对了。
        // 因为由于同样的问题，认为数据不存在
        //
        let adjust_max_timestamp = self.interval.get_close_unix_ms(max_timestamp);

        let expected_slots = (adjust_max_timestamp - min_timestamp) / interval_ms;
        let actual = self.count_distinct_between(
            &mut conn,
            &self.table_name,
            symbol,
            &self.time_column,
            min_timestamp,
            adjust_max_timestamp,
        )?;
        if actual == expected_slots {
            return Ok(vec![]);
        }

        let mut gaps: Vec<ValidationGap> = Vec::new();
        self.find_gaps_rec(
            symbol,
            &mut conn,
            &self.table_name,
            &self.time_column,
            &self.interval,
            min_timestamp,
            adjust_max_timestamp,
            &mut gaps,
        )?;

        // 合并相邻的缺失区间以便返回更简洁的结果（假设输入 gaps 无重叠）
        let merged_gaps = Self::merge_gaps(&mut gaps);
        if log::max_level() <= Level::Debug {
            for g in merged_gaps.iter() {
                match g {
                    ValidationGap::MissingData {
                        start_time,
                        end_time,
                        symbol,
                        ..
                    } => {
                        debug!(
                            "symbol:{} found gap: {} - {}, duration: {} mins",
                            symbol,
                            unix_2_readable(start_time),
                            unix_2_readable(end_time),
                            (end_time - start_time) / 60000
                        );
                    }
                    ValidationGap::UNKnowError { .. } => {}
                }
            }
        }
        if merged_gaps.is_empty() {
            return Ok(vec![]);
        }
        Ok(merged_gaps)
    }
}
#[async_trait]
impl ValidationStrategyTrait for SpotCheckStrategy {
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
        //防止检测到到当前周期。
        // 比如说现在11:36分，那么5分钟周期的，35已经开始了。但是可能还没结束，这样35可能被存入两次。
        let now = self.interval.get_now_close_unix_ms_utc() - 2 * self.interval.to_milliseconds();
        // 2. 并行检查每个 symbol（check_one_symbol 是同步 DB 操作，使用 spawn_blocking）
        let mut handles = Vec::new();
        let sem = Arc::new(Semaphore::new(10usize));
        for sym in symbols.into_iter() {
            let strategy = self.clone();
            let s = sym.clone();
            if self.ignore_symbols.read().await.is_ignored(&s) {
                continue;
            }
            let sem_clone = sem.clone();
            let h = tokio::task::spawn(async move {
                let jitter = Jitter::up_to(Duration::from_secs(30));
                tokio::time::sleep(jitter + Duration::ZERO).await;
                let _ = sem_clone.acquire().await.unwrap();
                strategy.check_one_symbol(&s, now)
            });
            handles.push(h);
        }

        // 3. 收集所有 gaps
        let mut all_gaps: Vec<ValidationGap> = Vec::new();
        for h in handles {
            match h.await {
                Ok(Ok(mut gaps)) => {
                    if !gaps.is_empty() {
                        if !self.ignore_symbols.write().await.plus_missing(&gaps) {
                            continue;
                        }
                        all_gaps.append(&mut gaps);
                    }
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
            info!("check {} completed,no gaps found", self.table_name);
            {
                self.ignore_symbols.write().await.reset();
            }
            return Ok(None);
        }

        let result = ValidationResult {
            id: yue::tools::get_snow_flake_id_u64(),
            strategy: self.name().to_string(),
            gaps: all_gaps,
            retry_count: 0,
            error: None,
        };

        Ok(Some(result))
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

///
/// 修复K线的问题
///
pub struct KlineGapRepairStrategy {
    symbol_type: SymbolType,
}

impl KlineGapRepairStrategy {
    pub fn spot() -> Self {
        Self {
            symbol_type: SymbolType::Spot,
        }
    }

    pub fn swap() -> Self {
        Self {
            symbol_type: SymbolType::Swap,
        }
    }
}

#[async_trait]
impl RepairStrategyTrait for KlineGapRepairStrategy {
    ///
    /// 1.loop req中的ValidationGap
    /// 2.通过HistoryFetcherFactory,来获取所有的kline
    /// 3.通过SPOT_STREAM_WRITER_ADDR，获得addr，发送消息去保存
    ///
    async fn repair(&self, req: RepairRequest) -> Result<(), YueError> {
        if req.gaps.is_empty() {
            return Ok(());
        }

        // 并发拉取：使用 Semaphore 控制并发量，避免同时发起过多请求
        let concurrency_limit = 10usize; // 可调整
        let sem = Arc::new(Semaphore::new(concurrency_limit));

        let mut handles = Vec::new();
        for gap in req.gaps.into_iter() {
            match gap {
                ValidationGap::MissingData {
                    symbol,
                    start_time,
                    end_time,
                    table: _,
                    ..
                } => {
                    let sem_clone = sem.clone();
                    let table = match self.symbol_type {
                        SymbolType::Spot => get_spot_kline_table(),
                        SymbolType::Swap => get_swap_kline_table(),
                        _ => {
                            error!("symbol type mismatch");
                            continue;
                        }
                    };
                    let kline_fetcher = match self.symbol_type {
                        SymbolType::Spot => HistoryFetcherImpl::kline(&SPOT_KLINE_HISTORY_COMMAND),
                        SymbolType::Swap => HistoryFetcherImpl::kline(&SWAP_KLINE_HISTORY_COMMAND),
                        _ => return Err(YueError::new("类型不支持")),
                    };
                    let symbol_type = self.symbol_type.clone();
                    // spawn 一个异步任务来处理该 gap
                    let handle = tokio::spawn(async move {
                        // 获取信号量许可
                        let _permit = sem_clone.acquire().await;

                        let base_param = CommonRequestBuilder::new(symbol.to_string(), 1000, HistoryInterval::FiveMinutes);
                        let saver = HistoryKlineSaver::new(table);
                        if let Err(e) = kline_fetcher
                            .get_all_kline_data(
                                base_param,
                                Some(HistoryInterval::FiveMinutes),
                                Some(start_time),
                                Some(end_time),
                                Some(saver),
                                true,
                            )
                            .await
                        {
                            //FUTURE: 未来通过工具通知远程
                            error!("repair {}:{} error,{}", symbol_type, symbol, e);
                        }
                    });
                    handles.push(handle);
                }
                other => {
                    debug!("repair: unsupported gap variant: {:?}", other);
                }
            }
        }

        for h in handles {
            match h.await {
                Err(join_err) => error!("system error {}", join_err),
                _ => {}
            }
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "kline_repair_strategy"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binance::db_consts::CREATE_SPOT_KLINE_TABLE;
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
        let symbol = "BTCUSDT";
        let interval = HistoryInterval::FiveMinutes;
        let interval_ms = interval.to_milliseconds() as i64;
        for i in 0..5 {
            let ts = t0 + i * interval_ms;
            let close_time = ts + interval_ms - 1;
            let sql = format!("INSERT INTO bn_spot_kline (id, symbol, candle_begin_time, open, high, low, close, volume, quote_volume, number_of_trades, taker_buy_base_asset_volume, taker_buy_quote_asset_volume, close_time, interval, first_trade_id, last_trade_id) VALUES ({}, '{}', {}, 0,0,0,0,0,0,0,0,0,{}, 1, 0, 0);", i, symbol, ts, close_time);
            conn.execute_batch(&sql)?;
        }

        let check_strategy = SpotCheckStrategy::spot_check_strategy(
            Some(db.clone()),
            100 * 24 * 60 * 60 * 1000, // 100 days retention
        );

        let start = t0 as u64;
        //真实环境，可能不是整点。所以比较来弄。
        let end = (t0 + (5 - 1) * interval_ms) as u64;
        let mut gaps: Vec<ValidationGap> = Vec::new();
        check_strategy.find_gaps_rec(symbol, &mut conn, "bn_spot_kline", "candle_begin_time", &interval, start, end, &mut gaps)?;
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
        let interval = HistoryInterval::FiveMinutes;
        let interval_ms = interval.to_milliseconds() as i64;
        let expected_symbol = "BTCUSDT";
        for i in 0..5 {
            if i == 2 {
                continue;
            }
            let ts = t0 + i * interval_ms;
            let close_time = ts + interval_ms - 1;
            let sql = format!("INSERT INTO bn_spot_kline (id, symbol, candle_begin_time, open, high, low, close, volume, quote_volume, number_of_trades, taker_buy_base_asset_volume, taker_buy_quote_asset_volume, close_time, interval, first_trade_id, last_trade_id) VALUES ({}, '{}', {}, 0,0,0,0,0,0,0,0,0,{}, 1, 0, 0);", i, expected_symbol, ts, close_time);
            conn.execute_batch(&sql)?;
        }
        let check_strategy = SpotCheckStrategy::spot_check_strategy(
            Some(db.clone()),
            100 * 24 * 60 * 60 * 1000, // 100 days retention
        );
        let start = t0 as u64;
        let end = (t0 + 5 * interval_ms) as u64;
        let mut gaps: Vec<ValidationGap> = Vec::new();
        check_strategy.find_gaps_rec(
            expected_symbol,
            &mut conn,
            "bn_spot_kline",
            "candle_begin_time",
            &interval,
            start,
            end,
            &mut gaps,
        )?;
        assert!(!gaps.is_empty(), "expected gaps but found none");
        // ensure one gap covers the missing slot at t0 + 2*interval
        let missing_ts = (t0 + 2 * interval_ms) as u64;
        let missing_close_ts = missing_ts + interval_ms as u64;
        let mut found = false;
        for g in gaps.iter() {
            if let ValidationGap::MissingData {
                symbol,
                start_time,
                end_time,
                ..
            } = g
            {
                if *start_time == missing_ts && *end_time == missing_close_ts {
                    assert_eq!(symbol, expected_symbol);
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "missing slot not detected in gaps: {:?}", gaps);
        Ok(())
    }

    /// 测试说明（单个缺失槽）:
    /// 场景: 数据库中有两列不同的symbol。BTCUSDT和ETHUSDT，在一段连续时间序列中刻意跳过中间一个时间槽（slot），其他槽均插入
    /// 输入: BTCUSDT 插入 0,1,3,4 四个槽的数据，缺少第 2 号槽，ETH为全部槽都是满的
    /// 预期: find_gaps_rec 能检测到一个 MissingData gap 覆盖缺失槽的时间点，且为BTCUSDT
    #[actix_rt::test]
    async fn test_find_gaps_rec_missing_gap_d_symbol() -> Result<(), YuError> {
        let db = create_memory_db_provider();
        let mut conn = db.acquire()?;
        conn.execute_batch(CREATE_SPOT_KLINE_TABLE)?;
        let t0: i64 = 1_700_000_000;
        let interval = HistoryInterval::FiveMinutes;
        let interval_ms = interval.to_milliseconds() as i64;
        let expected_symbol = "BTCUSDT";
        for i in 0..5 {
            let ts = t0 + i * interval_ms;
            let close_time = ts + interval_ms - 1;
            if i == 2 {
                debug!("missing data from {} to {}", ts, close_time);
                continue;
            }
            let sql = format!("INSERT INTO bn_spot_kline (id, symbol, candle_begin_time, open, high, low, close, volume, quote_volume, number_of_trades, taker_buy_base_asset_volume, taker_buy_quote_asset_volume, close_time, interval, first_trade_id, last_trade_id) VALUES ({}, '{}', {}, 0,0,0,0,0,0,0,0,0,{}, 1, 0, 0);", i, expected_symbol, ts, close_time);
            conn.execute_batch(&sql)?;
        }

        let expected_second_symbol = "ETHUSDT";
        for i in 0..5 {
            if i == 3 {
                continue;
            }
            let ts = t0 + i * interval_ms;
            let close_time = ts + interval_ms - 1;
            let sql = format!("INSERT INTO bn_spot_kline (id, symbol, candle_begin_time, open, high, low, close, volume, quote_volume, number_of_trades, taker_buy_base_asset_volume, taker_buy_quote_asset_volume, close_time, interval, first_trade_id, last_trade_id) VALUES ({}, '{}', {}, 0,0,0,0,0,0,0,0,0,{}, 1, 0, 0);", i, expected_second_symbol, ts, close_time);
            conn.execute_batch(&sql)?;
        }
        let check_strategy = SpotCheckStrategy::spot_check_strategy(
            Some(db.clone()),
            100 * 24 * 60 * 60 * 1000, // 100 days retention
        );
        let start = t0 as u64;
        let end = (t0 + (5 - 1) * interval_ms) as u64;
        let mut gaps: Vec<ValidationGap> = Vec::new();
        check_strategy.find_gaps_rec(
            expected_symbol,
            &mut conn,
            "bn_spot_kline",
            "candle_begin_time",
            &interval,
            start,
            end,
            &mut gaps,
        )?;
        assert!(!gaps.is_empty(), "expected gaps but found none");
        // ensure one gap covers the missing slot at t0 + 2*interval
        let missing_ts = (t0 + 2 * interval_ms) as u64;
        let missing_close_ts = missing_ts + interval_ms as u64;
        let mut found = false;
        for g in gaps.iter() {
            if let ValidationGap::MissingData {
                symbol,
                start_time,
                end_time,
                ..
            } = g
            {
                if *start_time == missing_ts && *end_time == missing_close_ts {
                    assert_eq!(symbol, expected_symbol);
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "missing slot not detected in gaps: {:?}", gaps);
        Ok(())
    }

    /// 测试说明（缺失多个槽）:
    /// 场景: 完整数据是 0-9 共10个槽，刻意缺失多个槽（2,3,6,7,9）
    /// 输入: 插入 0,1,4,5,8 五个槽的数据，缺少 2,3,6,7,9 五个槽
    /// 预期: find_gaps_rec 能检测到至少一个 MissingData gap 覆盖缺失槽的时间点
    #[actix_rt::test]
    async fn test_find_gaps_rec_missing_multi_gap() -> Result<(), YuError> {
        let db = create_memory_db_provider();
        let mut conn = db.acquire()?;
        conn.execute_batch(CREATE_SPOT_KLINE_TABLE)?;
        let t0: i64 = 1_700_000_000;
        let interval = HistoryInterval::FiveMinutes;
        let interval_ms = interval.to_milliseconds() as i64;
        let mut missing_indices = vec![2, 3, 6, 7, 9];
        let expected_symbol = "BTCUSDT";
        // 插入 0..9 的槽，跳过 missing_indices
        for i in 0..10 {
            if missing_indices.contains(&i) {
                continue;
            }
            let ts = t0 + i * interval_ms;
            let close_time = ts + interval_ms - 1;
            let sql = format!("INSERT INTO bn_spot_kline (id, symbol, candle_begin_time, open, high, low, close, volume, quote_volume, number_of_trades, taker_buy_base_asset_volume, taker_buy_quote_asset_volume, close_time, interval, first_trade_id, last_trade_id) VALUES ({}, '{}', {}, 0,0,0,0,0,0,0,0,0,{}, 1, 0, 0);", i, expected_symbol, ts, close_time);
            conn.execute_batch(&sql)?;
        }
        let check_strategy = SpotCheckStrategy::spot_check_strategy(
            Some(db.clone()),
            100 * 24 * 60 * 60 * 1000, // 100 days retention
        );

        let start = t0 as u64;
        let end = (t0 + 10 * interval_ms + 10) as u64;
        let mut gaps: Vec<ValidationGap> = Vec::new();
        check_strategy.find_gaps_rec(
            expected_symbol,
            &mut conn,
            "bn_spot_kline",
            "candle_begin_time",
            &interval,
            start,
            end,
            &mut gaps,
        )?;
        assert!(!gaps.is_empty(), "expected gaps but found none");

        // 验证每个缺失索引都能被检测到
        let mut found_missing = HashSet::new();

        for g in gaps.iter() {
            if let ValidationGap::MissingData {
                symbol,
                start_time,
                end_time,
                ..
            } = g
            {
                let idx = ((*start_time as i64 - t0) / interval_ms) as i64;
                assert!(missing_indices.contains(&idx), "检测到不该缺失的槽 idx:{}", idx);
                // 按值删除已发现的缺失索引（使用 retain）
                missing_indices.retain(|&v| v != idx);
                assert!(idx <= 9, "查询不再区间内的时间，idx:{}", idx);
                // 检查端点大小
                assert_eq!(*end_time, *start_time + interval_ms as u64);
                found_missing.insert(idx);
                assert_eq!(symbol, expected_symbol)
            }
        }
        assert!(missing_indices.is_empty(), "有些漏洞没有找出，{:?}", missing_indices);
        assert_eq!(gaps.len(), 5, "判断不对");
        for m in missing_indices.into_iter() {
            assert!(found_missing.contains(&(m)), "missing slot {} not detected, gaps: {:?}", m, gaps);
        }
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
                assert_eq!(*end_time, 399);
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
                    assert_eq!(*end_time, expected[i].1 - 1);
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
            assert_eq!(*end_time, 887);
        }

        // 空输入
        let mut empty: Vec<ValidationGap> = Vec::new();
        let merged_empty = SpotCheckStrategy::merge_gaps(&mut empty);
        assert!(merged_empty.is_empty(), "empty input should produce empty output");
    }
}
