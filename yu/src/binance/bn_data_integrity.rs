use crate::binance::bn_backend_service::{get_spot_kline_table, get_swap_kline_table};
use crate::binance::db_consts::BinanceTables;
use crate::binance::history::HistoryKlineSaver;
use crate::data_integrity::check::{BinarySearchDS, DuckDBBinarySearchDataImpl, ValidationStrategyTrait, binary_search_gap};
use crate::data_integrity::models::{RepairRequest, ValidationGap, ValidationResult};
use crate::data_integrity::repair::RepairStrategyTrait;
use crate::duck_db::DuckDBDSProvider;
use crate::duck_db_tables::DuckDbTableTrait;
use crate::errors::YuError;
use async_trait::async_trait;
use governor::Jitter;
use li::tools::time::unix_2_readable;
use log::{Level, debug, error, info, trace};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, Semaphore};
use yue::binance::bn_models::common::SymbolType;
use yue::binance::bn_restful_commands::{SPOT_KLINE_HISTORY_COMMAND, SWAP_KLINE_HISTORY_COMMAND};
use yue::binance::restful_func::{CommonRequestBuilder, HistoryFetcherImpl, HistoryFetcherTrait};
use yue::errors::YueError;
use yue::models::HistoryInterval;
use yue::query_message::DataSourceProviderTrait;

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
    binary_search_ds: BinarySearchDS,
    db_provider: DuckDBDSProvider,
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
    pub fn spot_check_strategy(db_source: Option<DuckDBDSProvider>, data_retention_time: u64) -> Self {
        let db_provider = db_source.unwrap_or_else(|| DuckDBDSProvider::default());
        let binary_search_ds = DuckDBBinarySearchDataImpl::new(
            db_provider.clone(),
            BinanceTables::SpotKline.table_name(),
            "candle_begin_time".to_string(),
            "symbol".to_string(),
        );
        Self {
            binary_search_ds,
            db_provider,
            interval: HistoryInterval::FiveMinutes, // 5分钟
            ignore_symbols: Arc::new(RwLock::new(IgnoreSymbols::default())),
            data_retention_time,
            name: BN_SPOT_KLINE_CHECK.to_string(),
        }
    }

    pub fn swap_check_strategy(db_source: Option<DuckDBDSProvider>, data_retention_time: u64) -> Self {
        let db_provider = db_source.unwrap_or_else(|| DuckDBDSProvider::default());
        let binary_search_ds = DuckDBBinarySearchDataImpl::new(
            db_provider.clone(),
            BinanceTables::SwapKline.table_name(),
            "candle_begin_time".to_string(),
            "symbol".to_string(),
        );
        Self {
            db_provider,
            binary_search_ds,
            interval: HistoryInterval::FiveMinutes, // 5分钟
            ignore_symbols: Arc::new(RwLock::new(IgnoreSymbols::default())),
            data_retention_time,
            name: BN_SWAP_KLINE_CHECK.to_string(),
        }
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
        let binary_search_ds = &self.binary_search_ds.clone();
        trace!("check_one_symbol symbol: {} at {}", symbol, binary_search_ds.time_column());
        let conn = self
            .db_provider
            .acquire()
            .map_err(|e| YuError::new(&format!("when check {}, query db provider,e is {}", symbol, e)))?;
        let interval_ms = self.interval.to_milliseconds();

        // 使用参数化查询以避免注入，并安全获取 min/max
        let sql = format!(
            "SELECT MIN({}), MAX({}) FROM {} WHERE {} = ?",
            binary_search_ds.time_column(),
            binary_search_ds.time_column(),
            binary_search_ds.table_name(),
            binary_search_ds.symbol_column(),
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
        let actual = self
            .binary_search_ds
            .count_distinct_between(symbol, min_timestamp, adjust_max_timestamp)?;
        if actual == expected_slots {
            return Ok(vec![]);
        }

        let gaps: Vec<ValidationGap> = binary_search_gap(
            symbol,
            "SPOT",
            min_timestamp,
            adjust_max_timestamp,
            &self.interval,
            self.binary_search_ds.clone(),
        )?;
        if log::log_enabled!(Level::Debug) {
            for g in gaps.iter() {
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
                            unix_2_readable(&start_time),
                            unix_2_readable(&end_time),
                            (end_time - start_time) / 60000
                        );
                    }
                    ValidationGap::UNKnowError { .. } => {}
                }
            }
        }
        Ok(gaps)
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
                sym = self.binary_search_ds.symbol_column(),
                table = self.binary_search_ds.table_name()
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
            info!("check {} completed,no gaps found", self.binary_search_ds.table_name());
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
