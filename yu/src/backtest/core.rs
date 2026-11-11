//!
//! 本模块主要是用于回测的核心逻辑，主要包括以下几个方面：
//! 1. 回测的核心类和trait的定义。
//! 2. 通用的一些实现。
//!
//! # 设计
//! 其实也就三个核心，
//! 1. 获取和分发数据的DataFeedTrait
//! 2. 策略的执行和调度的StrategyTrait
//! 3. 回测的核心逻辑BacktestCoreTrait
//!

use crate::errors::YuError;
use async_trait::async_trait;
use futures::Stream;
use polars::prelude::DataFrame;

///
/// 做为回测的核心trait，主要是为了一个placeholder
///
/// 主要这一个回测，比较适合做成函数。是一次性的。但是有问题。就是说不同的回测，参数不同，统一回非常麻烦
/// 这个唯一的使用场景，也就是不同策略的混合。比如说一个期权策略和中性策略混合。但是这也是后期的功能。
///
///
/// ## 规则
/// 1. 所有的回测的参数，做为struct的属性放入。这样做，是为了方便后续的调度。
///
///
pub trait BacktestCoreTrait {
    type Result: Clone + Send;
    fn calculate(&self) -> Result<Self::Result, YuError>;
}

///
/// 其实关于策略，本质的问题和BacktestCoreTrait是一致的。
/// 但是不同类型的策略统一调度的场景还是挺多的。
/// 1. 实盘。
/// 2，回测时候的混合策略。
///
/// 现在想的到想要支持的策略
/// 1. 选股类策略，需要有全市场或者某个域下面，选币或者选股策略。
///     - CTA策略其实也可以简化为这类策略，可以看成是域只有一个选股
/// 2. 一些订单流类，或者拨头皮类策略，主要是做一些高频的策略。
/// 3. 期权类策略
///
///
/// # 业务设计
///
/// ## 向量模式和序列模式
/// 就是一把算出所有，还是一个tick，一个tick来算。
/// 其实前向量模式，只是快，而且实现简单，序列模式复杂，但是比较适合实盘。
/// 这里暂时考虑两个都支持吧。
/// 1， 回测如果两个都有，则是优选向量。
/// 2. 实盘有限选序列模式
///
/// ## 关于运行模式
/// 基于上面的想法，这里有两种运行模式，就是运行两种execute方法，tick_execute和batch_execute。前者是序列模式，后者是向量模式。
/// 而关于策略的其他参数，则是作为属性来配置。
///
#[async_trait]
pub trait SequenceStrategyTrait {
    type Result: Clone + Send;

    ///
    /// 一次返回所有的结果，主要是为了向量模式的策略。
    ///
    async fn batch_execute(&self) -> Result<Vec<Self::Result>, YuError>;
}

#[async_trait]
pub trait VectorStrategyTrait {
    type Result: Clone + Send;
    ///
    /// 一次返回一个周期的结果。只是支持序列模式的策略。
    /// 1. 策略需要自己维护状态，
    /// 2. 每调用一次，则表示状态有一次变化。
    ///
    async fn tick_execute(&self) -> Result<Self::Result, YuError>;
}

///
/// 这里为了迎合策略的两种方式。所以来说，做了这种处理。
///
#[async_trait]
pub trait DataFeedTrait {
    type Entry: Clone + Send;

    async fn get_steam(&self) -> Result<impl Stream<Item = Self::Entry>, YuError>;
}

#[async_trait]
pub trait VectorDataFeedTrait {
    ///
    /// 这里不用而是用Dataframe，主要是在处理的时候，往往是多维度的。有时候处理要基于时间，有时候要基于instrument。
    /// 如果我用list，map，则需要通过不同的业务需求，进行调整。缺乏通用性。
    ///
    async fn get_all_data(&self) -> Result<DataFrame, YuError>;
}
