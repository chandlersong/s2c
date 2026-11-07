use async_trait::async_trait;

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait BinanceOptionApiTrait: Send + Sync {}
