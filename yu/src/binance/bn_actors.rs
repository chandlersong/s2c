use crate::binance::binance_db_consts::BinanceTables;
use crate::binance::models::po::DuckDBPO;
use crate::duck_db::get_connection;
use crate::errors::YuError;
use actix::{Actor, Addr, Context, Handler};
use duckdb::DropBehavior;
use log::error;
use std::marker::PhantomData;
use yue::errors::YueError;
use yue::query_message::{BatchInsert, Count, UNKNOWN_ROW};

pub mod table_query_message {}

///
/// 基于DuckDB对一张表
///
pub struct DuckDBOneTable<P: DuckDBPO> {
    table: BinanceTables,
    // use a raw pointer PhantomData to avoid imposing auto trait bounds (like Unpin) on P
    _marker: PhantomData<*const P>,
}

impl<P: DuckDBPO> DuckDBOneTable<P> {
    pub fn new(table: BinanceTables) -> Self {
        DuckDBOneTable { table, _marker: PhantomData }
    }

    pub fn start_new(table: BinanceTables) -> Addr<Self> {
        DuckDBOneTable { table, _marker: PhantomData }.start()
    }

    fn write_batch(&self, data: Vec<P>) -> Result<usize, YueError> {
        if data.is_empty() {
            return Ok(0);
        }
        let res = data.len();
        let mut conn = get_connection().map_err(|_e| YueError::CustomError(String::from("Failed to connect to DuckDB")))?;
        let mut tx = conn
            .transaction()
            .map_err(|_e| YueError::CustomError(String::from("Failed to create duckDB transaction")))?;
        tx.set_drop_behavior(DropBehavior::Commit);
        let mut appender = match tx.appender(&self.table.table_name()) {
            Ok(a) => a,
            Err(e) => {
                error!("Failed to create appender for table {}: {}", self.table.table_name(), e);
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
            error!("Failed to flush appender for table {}: {}", self.table.table_name(), e);
            return Err(YueError::CustomError("Failed to flush appender".to_string()));
        };
        Ok(res)
    }

    fn is_empty(&self) -> Result<isize, YuError> {
        // 如果表没有提供 query_lastest_record SQL，则认为没有可查询的最新记录
        let query_sql_opt = self.table.count_records();
        if query_sql_opt.is_none() {
            return Err(YuError::CustomError(format!(
                "Table {:?} does not support counting records",
                self.table.table_name()
            )));
        }

        let sql = query_sql_opt.unwrap();
        let conn = get_connection()?;

        let mut stmt = conn.prepare(sql.as_str())?;
        let mut rows = stmt.query([])?;

        if let Some(row) = rows.next()? {
            let count: isize = row.get(0)?;
            return Ok(count);
        }

        Err(YuError::CustomError(format!("Table {:?} 数据库访问失败", self.table.table_name())))
    }
}

impl<P: DuckDBPO> Actor for DuckDBOneTable<P> {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        // 目前不维护内部缓冲，保留入口以便未来扩展为定时刷新逻辑
        log::info!("DuckDBOneDataWriter actor started for table: {}", self.table.table_name());
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        log::info!("DuckDBOneDataWriter actor stopped for table: {}", self.table.table_name());
    }
}

impl<P: DuckDBPO> actix::Supervised for DuckDBOneTable<P> {}

impl<P: DuckDBPO> Handler<Count> for DuckDBOneTable<P> {
    type Result = isize;
    fn handle(&mut self, _: Count, _: &mut Self::Context) -> Self::Result {
        // reuse existing is_empty which still uses self.table
        self.is_empty().unwrap_or(UNKNOWN_ROW)
    }
}

impl<P> Handler<BatchInsert<<P as DuckDBPO>::Source>> for DuckDBOneTable<P>
where
    P: DuckDBPO + Clone + 'static,
    P::Source: Clone + 'static,
{
    type Result = Result<usize, YueError>;

    fn handle(&mut self, msg: BatchInsert<<P as DuckDBPO>::Source>, _ctx: &mut Self::Context) -> Self::Result {
        let symbol_opt = msg.symbol.as_deref();
        let po_vec = msg.data.into_iter().map(|v| P::from_source(symbol_opt, &v)).collect::<Vec<P>>();

        // write_batch already returns Result<usize, YueError>
        self.write_batch(po_vec)
    }
}

// impl Handler<BatchInsert<BinanceKline>> for DuckDBOneTableDataWriter {
//     type Result = Result<usize, YueError>;
//
//     /// 关于BatchInsert的处理逻辑：
//     /// 1. 每个单独写吧。因为写统一的有点麻烦。
//     /// 2. 这里是同步的，调用端要做错误的处理，主要的原因在于很难统一。
//     ///     比如说日常更新，有实效要求。最好报错。但是初始化的条件，瞬时压力很大，而且重试即可。
//     ///
//     fn handle(&mut self, msg: BatchInsert<BinanceKline>, _ctx: &mut Self::Context) -> Self::Result {
//         let symbol_str = msg.symbol.unwrap();
//         let symbol = Some(symbol_str.as_ref());
//         let history_po_vec = msg.data.iter().map(|v| convert_to_po(symbol.clone(), v)).collect::<Vec<KlinePo>>();
//         self.write_batch(history_po_vec)
//     }
// }
//
// impl Handler<BatchInsert<FundingRate>> for DuckDBOneTableDataWriter {
//     type Result = Result<usize, YueError>;
//
//     fn handle(&mut self, msg: BatchInsert<FundingRate>, _ctx: &mut Self::Context) -> Self::Result {
//         let symbol_str = msg.symbol.unwrap();
//         let symbol = Some(symbol_str.as_ref());
//         let funding_rate_po_vec = msg
//             .data
//             .iter()
//             .map(|v| FundingRatePo::from_source(symbol.clone(), v))
//             .collect::<Vec<FundingRatePo>>();
//         self.write_batch(funding_rate_po_vec)
//     }
// }
