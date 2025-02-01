use crate::binance::bn_models::MiniTicker;
use crate::models::DashBoard;
use async_trait::async_trait;
use moka::future::Cache;
use rand::{Rng, SeedableRng};


#[derive(Clone)]
pub struct BinanceTickDashBoard {
    cache: Cache<String, MiniTicker>,
}

impl BinanceTickDashBoard {
    pub fn new() -> Self {
        BinanceTickDashBoard {
            cache: Cache::new(500)
        }
    }
}

#[async_trait]
impl DashBoard<MiniTicker> for BinanceTickDashBoard {
    async fn set_value(&mut self, key: String, value: MiniTicker) {
        self.cache.insert(key, value).await;
    }

    async fn get_value(&mut self, key: String) -> MiniTicker {
        self.cache.get(&key).await.unwrap()
    }
}


#[cfg(test)]
mod tests {
    use crate::binance::bn_cache::BinanceTickDashBoard;
    use crate::binance::bn_tools::create_mock_mini_ticker;
    use crate::models::DashBoard;

    #[tokio::test]
    async fn test_bn_cache_normal() {
        let mut dashboard = BinanceTickDashBoard::new();
        let mini_ticker = create_mock_mini_ticker("a1".to_string(), 1.0);

        dashboard.set_value("a1".to_string(), mini_ticker.clone()).await;

        let actual = dashboard.get_value("a1".to_string()).await;
        assert_eq!(mini_ticker, actual);
    }

}
