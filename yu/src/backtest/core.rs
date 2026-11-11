//!
//! 本模块主要是用于回测的核心逻辑，主要包括以下几个方面：
//! 1. 回测的核心类和trait的定义。
//! 2. 通用的一些实现。
//!
//!  其实策略，本质的问题和BacktestCoreTrait是一致的。
//! 但是不同类型的策略统一调度的场景还是挺多的。
//! 1. 实盘。
//! 2，回测时候的混合策略。
//!
//! 现在想的到想要支持的策略
//! 1. 选股类策略，需要有全市场或者某个域下面，选币或者选股策略。
//!     - CTA策略其实也可以简化为这类策略，可以看成是域只有一个选股
//! 2. 一些订单流类，或者拨头皮类策略，主要是做一些高频的策略。
//! 3. 期权类策略
//!
//!
//! # 业务设计
//
//! ## 向量模式和序列模式
//! 就是一把算出所有，还是一个tick，一个tick来算。
//! 其实前向量模式，只是快，而且实现简单，序列模式复杂，但是比较适合实盘。
//! 这里暂时考虑两个都支持吧。
//! 1， 回测如果两个都有，则是优选向量。
//! 2. 实盘有限选序列模式
//!
//! ## 关于运行模式
//! 基于上面的想法，这里有两种运行模式，就是运行两种execute方法，tick_execute和batch_execute。前者是序列模式，后者是向量模式。
//! 而关于策略的其他参数，则是作为属性来配置。
//!
//! ## 没有定义StrategyTrait
//! 主要是策略其实很灵活，定义也是一个空的trait，
//! 如果需要不同类型策略的混用，其实需要定义多个Engine，因为不同类型的策略，feed，return这些完全不一样。
//! 如果是相同策略的混用，完全可以Strategy里面套用Strategy。没有必要去定义一个空的trait。
//!
//! 实际使用中，相同类型的策略，最好是一个文件，比如中性，剥头皮策略应该用相同的文件。
//!
//! 实际使用中，自定义的策略往往是
//! 1. 配置文件，比如一些纯因子的选币类。
//! 2. 一个方法的hook，比如一些CTA策略，或者剥头皮策略。
//! 这些都没有办法做到统一。
//! 但是策略的本质是计算Positon。这才是最关键的。这里也无法处理。
//!
//!

use crate::errors::YuError;
use async_trait::async_trait;
use futures::Stream;
use polars::prelude::DataFrame;

///
/// 作为一个总协调的地方，这里偏向一个placeholder的地方。
///
#[async_trait]
pub trait EngineTrait {
    type Result: Clone + Send;

    ///
    /// 一次返回所有的结果，主要是为了向量模式的策略。
    ///
    async fn batch_execute(&self) -> Result<Vec<Self::Result>, YuError>;
}

///
/// 做为回测的核心trait，主要是为了一个placeholder
///
/// 主要是为了计算回撤的回报。因为本质是根据仓位信息和市场信息，计算出回报。然后根据回报计算出回撤。
/// 无论是向量模式还是序列模式，都是一样的。
/// 向量模式，是一个数组，calculate会被调用一次。回报在这里来组合
/// 序列模式，是一次一次的调用calculate，由外围来整合成完整的回报。
///
///
/// ## 规则
/// 1. 所有的回测的参数，做为struct的属性放入。这样做，是为了方便后续的调度。
///
///
pub trait ReturnCalculateTrait {
    type Position: Clone + Send;
    type Market: Clone + Send;
    type Result: Clone + Send;

    fn calculate(&self, position_data: Self::Position, market_data: Self::Market) -> Result<Self::Result, YuError>;
}

///
/// 这里为了迎合策略的两种方式。所以来说，做了这种处理。
///
#[async_trait]
pub trait SequenceDataFeedTrait {
    type Entry: Clone + Send;

    async fn get_steam(&self) -> Result<impl Stream<Item = Self::Entry>, YuError>;
}

#[async_trait]
pub trait VectorDataFeedTrait {
    ///
    /// 这里不用而是用Dataframe，主要是在处理的时候，往往是多维度的。有时候处理要基于时间，有时候要基于instrument。
    /// 如果我用list，map，则需要通过不同的业务需求，进行调整。缺乏通用性。
    ///
    /// FUTURE:
    /// 1. 优化流程，首先，写成本地文件parquet，然后在用polars进行lazy读取
    ///
    /// NOTE:
    /// 1. 币安的数据，1000小时大概10M，5000小时12.66 MB。时间倒是一样的。
    ///
    async fn get_all_data(&self) -> Result<DataFrame, YuError>;
}
