use crate::errors::YueError;
use tokio::sync::{mpsc, oneshot};
///
/// 这里mod主要是为了抽象一些数据库的操作。
/// 因为在实际操作中，我想这个对于数据库是解耦的。但是有一些操作还是有必要的。比如insert等操作。
/// 借助actix，则可以做到这一点。这里会定义一些基本的数据操作的message
///

pub type DataSourceExecutor<V> = mpsc::Sender<QueryCommand<V>>;

pub enum QueryCommand<V: Send> {
    GetCount(oneshot::Sender<Result<usize, YueError>>), // 查询数量，返回 usize,如果-1，表示查询出错
    BatchInsert(BatchInsertPayload<V>),
    Insert(InsertPayload<V>),
    // Add more commands as needed...
}

///
/// 查询有多少条记录
///
pub struct Count {}

pub struct InsertPayload<V: Send> {
    pub symbol: Option<String>,
    pub data: V,
    pub callback: Option<oneshot::Sender<Result<usize, YueError>>>,
}

impl<V: Send> InsertPayload<V> {
    pub fn new(symbol: Option<String>, data: V, result_tx: oneshot::Sender<Result<usize, YueError>>) -> Self {
        Self {
            symbol,
            data,
            callback: Some(result_tx),
        }
    }

    pub fn new_no_replay(symbol: Option<String>, data: V) -> Self {
        Self {
            symbol,
            data,
            callback: None,
        }
    }
}

///
/// 批量插入数据，返回的是插入多少条
///
pub struct BatchInsertPayload<V: Send> {
    pub symbol: Option<String>,
    pub data: Vec<V>,
    pub callback: Option<oneshot::Sender<Result<usize, YueError>>>,
}

impl<V: Send> BatchInsertPayload<V> {
    pub fn new(symbol: Option<String>, data: Vec<V>, result_tx: oneshot::Sender<Result<usize, YueError>>) -> Self {
        Self {
            symbol,
            data,
            callback: Some(result_tx),
        }
    }

    pub fn new_no_replay(symbol: Option<String>, data: Vec<V>) -> Self {
        Self {
            symbol,
            data,
            callback: None,
        }
    }
}
