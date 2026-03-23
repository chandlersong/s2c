use crate::errors::YueError;
///
/// 这里mod主要是为了抽象一些数据库的操作。
/// 因为在实际操作中，我想这个对于数据库是解耦的。但是有一些操作还是有必要的。比如insert等操作。
/// 借助actix，则可以做到这一点。这里会定义一些基本的数据操作的message
///
use actix::Message;

///
/// 查询有多少条记录
///
#[derive(Message)]
#[rtype(result = "isize")]
pub struct Count {}

impl Count {
    pub fn new() -> Self {
        Self {}
    }
}

pub const UNKNOWN_ROW: isize = -1;

///
/// 批量插入数据，返回的是插入多少条
///
pub struct BatchInsert<V> {
    pub symbol: Option<String>,
    pub data: Vec<V>,
}

impl<V> BatchInsert<V> {
    pub fn new(symbol: Option<String>, data: Vec<V>) -> Self {
        Self { symbol, data }
    }
}
impl<V> Message for BatchInsert<V> {
    type Result = Result<usize, YueError>;
}
