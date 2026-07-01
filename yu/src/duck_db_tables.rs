use crate::duck_db::{DBProvider, DuckDBPO, DuckDbConnection};
use crate::errors::YuError;
use duckdb::DropBehavior;
use li::tools::time::unix_time_now_u64_utc;
use log::error;
use polars::prelude::DataFrame;
use serde_json::Value;
use std::collections::HashMap;
use std::format;
use std::marker::PhantomData;
use std::time::Duration;
use tokio::sync::mpsc;
use yue::errors::YueError;
use yue::query_message::{ExecuteSQLPayload, QueryCommand};

pub type DuckTableTableChannel<P> = mpsc::Sender<QueryCommand<P>>;

pub trait DuckDbTableTrait: Send + Clone + 'static {
    fn table_name(&self) -> String;
    fn create_table_statement(&self) -> String;
    fn query_lastest_record(&self) -> Option<String>;
    fn count_records(&self) -> Option<String> {
        format!("select count(*) from {}", self.table_name()).into()
    }
}
///
/// 基于DuckDB对一张表
/// 为了简化现有的代码。大致为两层。
/// 1. 外层负责VO->PO的转换。因为这是一个业务相关的。而且会有很多不同的变种。
/// 2. 内层，也就该类，主要则PO的操作。
///
pub struct DuckDBOneTable<P: DuckDBPO, T: DuckDbTableTrait> {
    table: T,
    // use a raw pointer PhantomData to avoid imposing auto trait bounds (like Unpin) on V and P
    // PhantomData only accepts one type parameter; use a tuple to hold multiple types.
    flush_interval: Duration,
    flush_count: usize,
    _marker: PhantomData<P>,
    db_provider: DBProvider,
}

// FUTURE: 以后做成根据具体表的变换。纯技术需求。
const DB_CHANNEL_CAPACITY: usize = 1000;
const FLUSH_INTERVAL: Duration = Duration::from_secs(1);

impl<P: DuckDBPO, T: DuckDbTableTrait> DuckDBOneTable<P, T> {
    pub fn start_new(table: T, db_provider: Option<DBProvider>) -> DuckTableTableChannel<P> {
        let provider = db_provider.unwrap_or_else(|| DBProvider::default());
        DuckDBOneTable {
            table,
            flush_interval: FLUSH_INTERVAL,
            flush_count: 600, //主要对应的是kline的根数。
            _marker: PhantomData::<P>,
            db_provider: provider,
        }
        .start_listen()
    }
    fn write_batch(mut conn: DuckDbConnection, table: T, data: Vec<P>) -> Result<usize, YueError> {
        if data.is_empty() {
            return Ok(0);
        }
        let res = data.len();
        let mut tx = conn
            .transaction()
            .map_err(|_e| YueError::CustomError(String::from("Failed to create duckDB transaction")))?;
        tx.set_drop_behavior(DropBehavior::Commit);
        let mut appender = match tx.appender(&table.table_name()) {
            Ok(a) => a,
            Err(e) => {
                error!("Failed to create appender for table {}: {}", table.table_name(), e);
                return Err(YueError::CustomError("Failed to create appender".to_string()));
            }
        };

        for po in data {
            if let Err(e) = appender.append_row(po.to_params()) {
                error!("Failed to append row {:?}: {}", po, e);
                return Err(YueError::CustomError("Failed to append row".to_string()));
            }
        }
        if let Err(e) = appender.flush() {
            error!("Failed to flush appender for table {}: {}", table.table_name(), e);
            return Err(YueError::CustomError("Failed to flush appender".to_string()));
        };
        Ok(res)
    }

    pub fn get_connection(&self) -> Result<DuckDbConnection, YuError> {
        self.db_provider.acquire()
    }

    pub fn query_to_dataframe(conn: DuckDbConnection, payload: &ExecuteSQLPayload) -> Result<DataFrame, YueError> {
        let mut stmt = conn.prepare(payload.sql.as_str()).map_err(|e| YueError::CustomError(e.to_string()))?;

        // 如果 params 是 None，直接执行无参数查询
        let polars_iter = if let Some(params_map) = &payload.params {
            // 为了避免返回对临时值的引用，我们把转换后的参数放到一个 Vec<Box<dyn ToSql>> 中持有
            // 先把所有可转换的参数收集到一个 vec 中（避免在持有引用时再扩容/移动）
            let mut kvs: Vec<(&str, Box<dyn duckdb::ToSql>)> = Vec::with_capacity(params_map.len());
            for (key, value) in params_map.iter() {
                if matches!(value, Value::Null) {
                    continue;
                }
                if let Some(b) = to_sql_value(value) {
                    kvs.push((key.as_str(), b));
                }
            }
            // 然后从 kvs 构建最终的引用 map，kvs 在此作用域内保持不变，因此引用是安全的
            let mut filtered: HashMap<&str, &dyn duckdb::ToSql> = HashMap::with_capacity(kvs.len());
            for (k, boxed) in kvs.iter() {
                filtered.insert(*k, boxed.as_ref());
            }

            stmt.query_polars(&filtered).map_err(|e| YueError::CustomError(e.to_string()))?
        } else {
            stmt.query_polars([]).map_err(|e| YueError::CustomError(e.to_string()))?
        };

        // 取出第一个 DataFrame（通常查询结果只有一块）
        match polars_iter.into_iter().next() {
            Some(df) => Ok(df.into()),
            None => Ok(DataFrame::default()), // 返回空 DataFrame
        }
    }
    fn count_table(conn: DuckDbConnection, table: T) -> Result<usize, YueError> {
        // 如果表没有提供 query_lastest_record SQL，则认为没有可查询的最新记录
        let query_sql_opt = table.count_records();
        if query_sql_opt.is_none() {
            return Err(YueError::CustomError(format!(
                "Table {:?} does not support counting records",
                table.table_name()
            )));
        }

        let sql = query_sql_opt.unwrap();
        let mut stmt = conn.prepare(sql.as_str()).map_err(|e| YueError::CustomError(e.to_string()))?;
        let mut rows = stmt.query([]).map_err(|e| YueError::CustomError(e.to_string()))?;

        if let Some(row) = rows.next().map_err(|e| YueError::CustomError(e.to_string()))? {
            let count: usize = row.get(0).map_err(|e| YueError::CustomError(e.to_string()))?;
            return Ok(count);
        }

        Err(YueError::CustomError(format!("Table {:?} 数据库访问失败", table.table_name())))
    }

    fn flush_data(conn: DuckDbConnection, table: T, data: Vec<P>) {
        tokio::spawn(async move { Self::write_batch(conn, table, data) });
    }

    ///
    /// 启动一个线程，监听rx发来的的消息。发来的rx都是yue::query_message::QueryCommand
    /// # GetCount
    /// 获取当前数据库的行数。调用本类的方法count_table。然后通过里面的channel返回。
    ///
    /// # BatchInsert
    /// 线程内维护一个cache。然后以下两个条件判断是否要写入。
    /// 1. 上次存入和这次相差1s。
    /// 2. cache里面有超过100条。按照kline来考虑。
    ///
    /// # 定时检查
    /// 1. 比如BatchInsert，定期检查一下，如果1s内没有收到消息，也要能够保存。
    ///
    pub fn start_listen(self) -> DuckTableTableChannel<P> {
        let (tx, mut rx) = mpsc::channel(DB_CHANNEL_CAPACITY);
        let table = self.table.clone();
        let flush_interval = self.flush_interval;
        let flush_count = self.flush_count;
        tokio::spawn(async move {
            let mut single_cache = vec![];
            let mut cache_count = 0;
            let mut last_flush_time = unix_time_now_u64_utc();
            loop {
                tokio::select! {
                    // 处理接收消息；注意 rx.recv() 返回 None 表示所有 sender 已关闭，应退出循环
                    msg = rx.recv() => {
                        match msg {
                            Some(cmd) => {
                                match cmd {
                                    QueryCommand::GetCount(resp_tx) => {
                                         //统计行数
                                         match self.get_connection(){
                                            Ok(conn) => {
                                                  let count_result = Self::count_table(conn,table.clone());
                                                  if let Err(_unsent) = resp_tx.send(count_result) {
                                                    error!("在发送查询{}数量的时候出错：receiver 已关闭。", table.table_name());
                                                 }
                                            }
                                            Err(e) => {
                                                  error!("Failed to to get connection when count table,table is {},error is {}",table.table_name(),e);
                                            }
                                        };

                                    }
                                    QueryCommand::BatchInsert(payload) => {
                                        //批量保存

                                        match self.get_connection(){
                                            Ok(conn) => {
                                                let insert_result = Self::write_batch(conn,table.clone(), payload.data);
                                                if let Some(callback) =payload.callback{
                                                    if let Err(_unsent) = callback.send(insert_result) {
                                                        error!("在发送批量查询{}数量的时候出错：receiver 已关闭。", table.table_name());
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                  error!("Failed to to get connection when BatchInsert,table is {},error is {}",table.table_name(),e);
                                            }
                                        };

                                    }
                                    QueryCommand::Insert(payload) => {
                                         //单条保存，单条保存的逻辑
                                         // 1. 统一保存到cache里面去。下面两种状态刷新cache。
                                         //    1. 记录满500条。主要是为了保存k线。
                                         //    2. 上次刷新时间过了2s。
                                         // 2. 保存启动一条线程。
                                        single_cache.push(payload.data);
                                        let now = unix_time_now_u64_utc();
                                        cache_count = cache_count +1;
                                        let cond1 = (now - last_flush_time) < flush_interval.as_millis() as u64;
                                        let cond2 = cache_count >= flush_count;
                                        if cond1||cond2 {
                                            if single_cache.len() > 0 {
                                                  match self.get_connection(){
                                                    Ok(conn) => {
                                                        Self::flush_data(conn,table.clone(),single_cache);
                                                            single_cache = vec![];
                                                            cache_count = 0;
                                                            last_flush_time = now;
                                                        }
                                                    Err(e) => {
                                                          error!("Failed to to get connection when insert,table is {},error is {}",table.table_name(),e);
                                                    }
                                                };


                                            }
                                        };
                                    }QueryCommand::ExecuteSQL(payload) => {

                                          match self.get_connection(){
                                            Ok(conn) => {
                                                    let result = Self::query_to_dataframe(conn,&payload);
                                                    if let Some(callback) = payload.callback {
                                                         if let Err(_unsent) = callback.send(result) {
                                                             error!("在发送查询{}数量的时候出错：receiver 已关闭。", table.table_name());
                                                         }
                                                    }
                                            }
                                            Err(e) => {
                                                  error!("Failed to to get connection when ExecuteSQL,table is {},error is {}",table.table_name(),e);
                                            }
                                        };

                                    }}
                            }
                            None => {
                                // channel 关闭，退出后台任务
                                log::info!("DuckDBOneTable 后台任务因为所有 senders 被关闭而退出，table={}", table.table_name());
                                break;
                            }
                        }
                    }
                    _ = tokio::time::sleep(flush_interval) => {
                        // 可在此处执行周期性 flush 或维护逻辑
                        let now = unix_time_now_u64_utc();
                        if now - last_flush_time > flush_interval.as_millis() as u64 && !single_cache.is_empty(){
                              match self.get_connection(){
                                Ok(conn) => {
                                     Self::flush_data(conn,table.clone(),single_cache);
                                     single_cache = vec![];
                                     cache_count = 0;
                                     last_flush_time = now;
                                }
                                Err(e) => {
                                    error!("Failed to to get connection when flush data,table is {},error is {}",table.table_name(),e);
                                }
                              };

                        }
                    }
                }
            }
        });
        tx
    }
}

// 辅助函数：把 serde_json::Value 转为 duckdb::ToSql
fn to_sql_value(value: &Value) -> Option<Box<dyn duckdb::ToSql>> {
    match value {
        Value::Bool(b) => Some(Box::new(*b) as Box<dyn duckdb::ToSql>),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(Box::new(i) as Box<dyn duckdb::ToSql>)
            } else if let Some(f) = n.as_f64() {
                Some(Box::new(f) as Box<dyn duckdb::ToSql>)
            } else if let Some(u) = n.as_u64() {
                // prefer i64 when possible, otherwise use f64 fallback; here u64 -> i64 may overflow, so use f64
                Some(Box::new(u as f64) as Box<dyn duckdb::ToSql>)
            } else {
                None
            }
        }
        Value::String(s) => Some(Box::new(s.clone()) as Box<dyn duckdb::ToSql>),
        Value::Array(_) | Value::Object(_) => {
            // 将复杂类型序列化为 JSON 字符串传入（可以在 SQL 中解析或当作文本存储）
            match serde_json::to_string(value) {
                Ok(s) => Some(Box::new(s) as Box<dyn duckdb::ToSql>),
                Err(_) => None,
            }
        }
        Value::Null => None,
    }
}
