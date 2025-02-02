use crate::tools::FrequencyReducer;
use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
use moka::future::Cache;
use std::collections::HashMap;
use tokio::sync::broadcast;

#[cfg_attr(test, automock)]
#[async_trait]
pub trait DashBoard<T: Send> {
    /*
     像是价格，还有一些乱七八糟的信息这类，计划在缓存作为一个中转站。
     所以在这里对来类似于一个dashboard
    */
    async fn set_value(&mut self, key: String, value: T);

    async fn get_value(&mut self, key: String) -> Option<T>;
}

/// 这个估计用的不多。
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

    async fn get_value(&mut self, key: String) -> Option<T> {
        self.cache.get(&key).await
    }
}


///
/// 可以不停的调用 set_value,但是缓存的更新，会根据其设定的频率进行更新。
/// 比如你设定3s更新一次。
/// 那么1s的时候，设为1，2s的时候，设定为2，3s设定为3。那么1-3s不会存在于缓存中。
/// 第3s才会更新。
///
#[derive(Clone)]
pub struct FrequencyDashBoard<T: Send + Clone + Sync + 'static> {
    cache: Cache<String, T>,
    frequency_mill_seconds: u64,
    cache_tx: broadcast::Sender<(String, T)>,
    frequency_reducers: HashMap<String, FrequencyReducer<(String, T)>>
}

async fn cache_update<T: Send + Clone + Sync + 'static>(cache: Cache<String, T>, mut cache_rx: broadcast::Receiver<(String, T)>) {
    loop {
        if let Ok((key, value)) = cache_rx.recv().await {
            cache.insert(key, value).await;
        }
    }
}

/// TODO：
/// 1. 可以配置cache. channel的capacity和cache的都要。
impl<T: Send + Clone + Sync + 'static> FrequencyDashBoard<T> {
    pub async fn new(frequency_mill_seconds: u64) -> Self {
        let (tx, rx) = broadcast::channel(500);
        let cache: Cache<String, T> = Cache::new(500);
        let cache_4_update = cache.clone();
        tokio::spawn(async move {
            cache_update(cache_4_update, rx).await;
        }
        );
        FrequencyDashBoard {
            cache: cache.clone(),
            frequency_mill_seconds,
            cache_tx: tx,
            frequency_reducers: HashMap::new(),
        }
    }

    pub fn get_all_entries(&self)->Vec<T> {
        let mut res: Vec<T> = Vec::new();
        for (_, value) in &self.cache {
            res.push(value.clone());
        };
        res
    }
}

#[async_trait]
impl<T: Send + Clone + Sync + 'static> DashBoard<T> for FrequencyDashBoard<T> {
    async fn set_value(&mut self, key: String, value: T) {
        let frequency_reducer = self.frequency_reducers.get(&key).cloned();
        match frequency_reducer {
            Some(mut reducer) => {
                reducer.update((key, value)).await;
            }
            None => {
                let mut new_frequency_reducer = FrequencyReducer::new(self.cache_tx.clone(), self.frequency_mill_seconds).await;
                new_frequency_reducer.update((key.clone(), value)).await;
                self.frequency_reducers.insert(key, new_frequency_reducer);
            }
        }
    }

    async fn get_value(&mut self, key: String) -> Option<T> {
        self.cache.get(&key).await
    }
}

#[cfg(test)]
mod tests {
    use crate::cache::{DashBoard, FrequencyDashBoard, RealTimeDashBoard};
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_bn_realtime_board() {
        let mut dashboard = RealTimeDashBoard::new();

        dashboard.set_value("a1".to_string(), 1).await;

        let actual = dashboard.get_value("a1".to_string()).await;
        assert_eq!(1, actual.unwrap());
    }

    #[tokio::test]
    async fn test_bn_frequency_board_initial() {
        let mut dashboard = FrequencyDashBoard::new(100).await;

        dashboard.set_value("a1".to_string(), 1).await;
        let none_value = dashboard.get_value("a1".to_string()).await;
        assert_eq!(None, none_value); //测试没有更新
        sleep(Duration::from_millis(200)).await;
        let actual = dashboard.get_value("a1".to_string()).await;
        assert_eq!(1, actual.unwrap()); //测试初始化
    }


    #[tokio::test]
    async fn test_bn_frequency_board_replace() {
        let mut dashboard = FrequencyDashBoard::new(100).await;

        dashboard.set_value("a1".to_string(), 1).await;
        sleep(Duration::from_millis(200)).await;
        let actual = dashboard.get_value("a1".to_string()).await;
        assert_eq!(1, actual.unwrap());
        dashboard.set_value("a1".to_string(), 2).await;
        sleep(Duration::from_millis(200)).await;
        let second_value = dashboard.get_value("a1".to_string()).await;
        assert_eq!(2, second_value.unwrap());
    }

}

