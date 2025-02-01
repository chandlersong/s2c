use crate::models::DashBoard;
use async_trait::async_trait;
use moka::future::Cache;
use rand::{Rng, SeedableRng};


#[derive(Clone)]
pub struct RealTimeDashBoard<T: Send + Clone+ Sync + 'static> {
    cache: Cache<String, T>,
}

impl<T: Send+ Clone+ Sync + 'static> RealTimeDashBoard<T> {
    pub fn new() -> Self {
        RealTimeDashBoard {
            cache: Cache::new(500)
        }
    }
}

#[async_trait]
impl<T: Send+ Clone+ Sync + 'static> DashBoard<T> for RealTimeDashBoard<T> {
    async fn set_value(&mut self, key: String, value: T) {
        self.cache.insert(key, value).await;
    }

    async fn get_value(&mut self, key: String) -> T {
        self.cache.get(&key).await.unwrap()
    }
}


#[cfg(test)]
mod tests {
    use crate::binance::bn_cache::RealTimeDashBoard;
    use crate::binance::bn_tools::create_mock_mini_ticker;
    use crate::models::DashBoard;

    #[tokio::test]
    async fn test_bn_realtime_board() {
        let mut dashboard = RealTimeDashBoard::new();
        let mini_ticker = create_mock_mini_ticker("a1".to_string(), 1.0);

        dashboard.set_value("a1".to_string(), mini_ticker.clone()).await;

        let actual = dashboard.get_value("a1".to_string()).await;
        assert_eq!(mini_ticker, actual);
    }

}
