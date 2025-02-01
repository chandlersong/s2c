use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
use moka::future::Cache;


#[cfg_attr(test, automock)]
#[async_trait]
pub trait DashBoard<T: Send> {
    /*
     像是价格，还有一些乱七八糟的信息这类，计划在缓存作为一个中转站。
     所以在这里对来类似于一个dashboard
    */
    async fn set_value(&mut self, key: String, value: T);

    async fn get_value(&mut self, key: String) -> T;
}

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

#[derive(Clone)]
pub struct LatDashBoard<T: Send + Clone+ Sync + 'static> {
    cache: Cache<String, T>,
}

#[cfg(test)]
mod tests {
    use crate::cache::{DashBoard, RealTimeDashBoard};

    #[tokio::test]
    async fn test_bn_realtime_board() {
        let mut dashboard = RealTimeDashBoard::new();

        dashboard.set_value("a1".to_string(), 1).await;

        let actual = dashboard.get_value("a1".to_string()).await;
        assert_eq!(1, actual);
    }

}

