use crate::binance::binance_db_consts::BinanceTables;
use crate::binance::models::po::DuckDBPO;
use crate::duck_db::get_connection;
use duckdb::DropBehavior;
use log::error;
use std::marker::PhantomData;
use std::time::Duration;
use tokio::sync::mpsc;
use yue::errors::YueError;
use yue::query_message::{DataSourceExecutor, QueryCommand};

///
/// 基于DuckDB对一张表
/// 这里接收的应该是都Value，复制转换成最后存入数据库的PO
///
pub struct DuckDBOneTable<V, P: DuckDBPO<Source = V>> {
    table: BinanceTables,
    // use a raw pointer PhantomData to avoid imposing auto trait bounds (like Unpin) on V and P
    // PhantomData only accepts one type parameter; use a tuple to hold multiple types.
    _marker: PhantomData<(*const V, *const P)>,
}

// FUTURE: 以后做成根据具体表的变换。纯技术需求。
const DB_CHANNEL_CAPACITY: usize = 1000;

impl<V: Send + 'static, P: DuckDBPO<Source = V>> DuckDBOneTable<V, P> {
    pub fn start_new(table: BinanceTables) -> DataSourceExecutor<V> {
        DuckDBOneTable {
            table,
            _marker: PhantomData::<(*const V, *const P)>,
        }
        .start_listen()
    }
    fn write_batch(table: BinanceTables, data: Vec<P>) -> Result<usize, YueError> {
        if data.is_empty() {
            return Ok(0);
        }
        let res = data.len();
        let mut conn = get_connection().map_err(|_e| YueError::CustomError(String::from("Failed to connect to DuckDB")))?;
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

    fn count_table(table: BinanceTables) -> Result<usize, YueError> {
        // 如果表没有提供 query_lastest_record SQL，则认为没有可查询的最新记录
        let query_sql_opt = table.count_records();
        if query_sql_opt.is_none() {
            return Err(YueError::CustomError(format!(
                "Table {:?} does not support counting records",
                table.table_name()
            )));
        }

        let sql = query_sql_opt.unwrap();
        let conn = get_connection().map_err(|e| YueError::CustomError(e.to_string()))?;

        let mut stmt = conn.prepare(sql.as_str()).map_err(|e| YueError::CustomError(e.to_string()))?;
        let mut rows = stmt.query([]).map_err(|e| YueError::CustomError(e.to_string()))?;

        if let Some(row) = rows.next().map_err(|e| YueError::CustomError(e.to_string()))? {
            let count: usize = row.get(0).map_err(|e| YueError::CustomError(e.to_string()))?;
            return Ok(count);
        }

        Err(YueError::CustomError(format!("Table {:?} 数据库访问失败", table.table_name())))
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
    pub fn start_listen(self) -> DataSourceExecutor<V> {
        let (tx, mut rx) = mpsc::channel(DB_CHANNEL_CAPACITY);
        let table = self.table.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    // 处理接收消息；注意 rx.recv() 返回 None 表示所有 sender 已关闭，应退出循环
                    msg = rx.recv() => {
                        match msg {
                            Some(cmd) => {
                                match cmd {
                                    QueryCommand::GetCount(resp_tx) => {
                                        let count_result = Self::count_table(table.clone());
                                        if let Err(_unsent) = resp_tx.send(count_result) {
                                            error!("在发送查询{}数量的时候出错：receiver 已关闭。", table.table_name());
                                        }
                                    }
                                    QueryCommand::BatchInsert(payload) => {
                                        let symbol_opt = payload.symbol.as_deref();
                                        let po_vec = payload.data.into_iter().map(|v| P::from_source(symbol_opt, &v)).collect::<Vec<P>>();
                                        let insert_result = Self::write_batch(table.clone(), po_vec);
                                        if let Err(_unsent) = payload.callback.send(insert_result) {
                                            error!("在发送批量查询{}数量的时候出错：receiver 已关闭。", table.table_name());
                                        }
                                    }
                                }
                            }
                            None => {
                                // channel 关闭，退出后台任务
                                log::info!("DuckDBOneTable 后台任务因为所有 senders 被关闭而退出，table={}", table.table_name());
                                break;
                            }
                        }
                    }

                    // 空闲超时分支：5 分钟
                    _ = tokio::time::sleep(Duration::from_secs(300)) => {
                        log::debug!("DuckDBOneTable idle timeout (5m) for table {}", table.table_name());
                        // 可在此处执行周期性 flush 或维护逻辑
                    }
                }
            }
        });
        tx
    }
}
