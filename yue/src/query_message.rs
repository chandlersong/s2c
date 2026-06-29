use crate::errors::YueError;
use async_trait::async_trait;
use polars::prelude::DataFrame;
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::oneshot;

///
/// 这里mod主要是为了抽象一些数据库的操作。
/// 因为在实际操作中，我想这个对于数据库是解耦的。但是有一些操作还是有必要的。比如insert等操作。
/// 借助actix，则可以做到这一点。这里会定义一些基本的数据操作的message
///

#[async_trait]
pub trait DataSourceExecutorTrait<V: Send> {
    async fn execute(&self, command: QueryCommand<V>) -> Result<(), YueError>;
}

pub type DataSourceExecutor<V> = Box<dyn DataSourceExecutorTrait<V> + Send>;

pub enum QueryCommand<V: Send> {
    GetCount(oneshot::Sender<Result<usize, YueError>>), // 查询数量，返回 usize,如果-1，表示查询出错
    ExecuteSQL(ExecuteSQLPayload),                      // 查询数量，返回 usize,如果-1，表示查询出错
    BatchInsert(BatchInsertPayload<V>),
    Insert(InsertPayload<V>),
    // Add more commands as needed...
}

///
/// 查询有多少条记录
///
pub struct Count {}

pub struct ExecuteSQLPayload {
    pub sql: String,
    pub params: Option<HashMap<String, Value>>,
    pub callback: Option<oneshot::Sender<Result<DataFrame, YueError>>>,
}

impl ExecuteSQLPayload {
    pub fn new(sql: &str, params: Option<HashMap<String, Value>>, callback: oneshot::Sender<Result<DataFrame, YueError>>) -> Self {
        Self {
            sql: sql.to_string(),
            params,
            callback: Some(callback),
        }
    }

    pub fn new_no_replay(sql: &str, params: Option<HashMap<String, Value>>) -> Self {
        Self {
            sql: sql.to_string(),
            params,
            callback: None,
        }
    }
}

pub struct InsertPayload<V: Send> {
    pub data: V,
    pub callback: Option<oneshot::Sender<Result<usize, YueError>>>,
}

impl<V: Send> InsertPayload<V> {
    pub fn new(data: V, result_tx: oneshot::Sender<Result<usize, YueError>>) -> Self {
        Self {
            data,
            callback: Some(result_tx),
        }
    }

    pub fn new_all(data: V, callback: Option<oneshot::Sender<Result<usize, YueError>>>) -> Self {
        Self { data, callback }
    }

    pub fn new_no_replay(data: V) -> Self {
        Self { data, callback: None }
    }
}

///
/// 批量插入数据，返回的是插入多少条
///
pub struct BatchInsertPayload<V: Send> {
    pub data: Vec<V>,
    pub callback: Option<oneshot::Sender<Result<usize, YueError>>>,
}

impl<V: Send> BatchInsertPayload<V> {
    pub fn new(data: Vec<V>, result_tx: oneshot::Sender<Result<usize, YueError>>) -> Self {
        Self {
            data,
            callback: Some(result_tx),
        }
    }

    pub fn new_all(data: Vec<V>, callback: Option<oneshot::Sender<Result<usize, YueError>>>) -> Self {
        Self { data, callback }
    }

    pub fn new_no_replay(data: Vec<V>) -> Self {
        Self { data, callback: None }
    }
}
