use log::{debug, error, info, trace, warn};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use tokio::sync::{oneshot, RwLock};
use tokio::time::{sleep, timeout};

/// `SymbolRefresher` 是用于动态更新cache的代码。主要是用于监听Websocket的ticker的数据。
/// key为symbol+spot/swap，然后单独更新。
/// 一开始，其实没有多大的数量，币安的获取信息Spot和Swap分开的。当个在300左右，简化程序，所以写的简单点了。
///
/// 这样做的主要原因是：
/// 1. 因为是协程，应该不是很大。
/// 2. Websocket的数据是每秒推送。同时，只会推送过去1s的数据。如果每个都存，没有丢弃机制，可能会堵塞。
/// 3. 如果每一次推送，作为一个整体。因为那么些
///
///

type ShareCache<S> = Arc<RwLock<Option<S>>>;
type CacheSender<S> = Sender<(S, oneshot::Sender<S>)>;

async fn refresh_cache<S>(cache: &ShareCache<S>, value: S) {
    *cache.write().await = Some(value);
}
#[derive(Clone)]
pub struct SymbolRefresher<S: Send + Clone + Display> {
    cache: ShareCache<S>,
    symbol_key: String,
    cache_sender: CacheSender<S>,
}


impl<S: Send + Clone + Display> SymbolRefresher<S> {
    pub fn new(cache: ShareCache<S>, symbol_key: String, cache_sender: CacheSender<S>) -> Self {
        Self { cache, symbol_key, cache_sender }
    }


    async fn start(&self) {
        let mut rng = StdRng::from_entropy();
        loop {
            match self.cache.read().await.as_ref() {
                Some(cache) => {
                    let data = cache.clone();
                    let (rx, tx) = oneshot::channel();
                    self.cache_sender.send((data.clone(), rx)).await.expect("TODO: panic message");
                    match timeout(Duration::from_millis(200), tx).await {
                        Ok(response_result) => match response_result {
                            Ok(_) => { trace!("data refresher successfully") }
                            Err(e) => { error!("Error refresh cache: error:{}", e) }
                        },
                        Err(_) => { warn!("cache response timeout,{}",&data) }
                    }
                }
                _ => {
                    debug!("Cache not initialized");
                }
            }

            let sleep_time = rng.gen_range(750..=1000); // 生成 0 到 5 之间的随机数
            sleep(Duration::from_millis(sleep_time)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::binance::bn_cache::{refresh_cache, SymbolRefresher};
    use crate::binance::bn_models::MiniTicker;
    use crate::utils::setup_logger;
    use log::LevelFilter;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::{mpsc, RwLock};
    use tokio::time::sleep;

    #[tokio::test]
    pub async fn test_update_cache() {
        let source_data = MiniTicker {
            event_type: "bbc".to_string(),
            event_time: 1,
            symbol: "abc".to_string(),
            close: 0.1,
            open: 0.2,
            high: 0.3,
            low: 0.01,
            volume: 0.5,
            quote_volume: 0.6,
        };


        let data = MiniTicker {
            event_type: "abc".to_string(),
            event_time: 2,
            symbol: "bbc".to_string(),
            close: 0.11,
            open: 0.21,
            high: 0.13,
            low: 0.11,
            volume: 0.15,
            quote_volume: 0.16,
        };

        let source = Arc::new(RwLock::new(Some(source_data)));
        refresh_cache(&source, data.clone()).await;

        if let Some(value) = &*source.read().await {
            assert_eq!(value, &data, "数据更新不正确");
        } else {
            assert!(false, "cache not refresh!");
        };
    }


    #[tokio::test]
    pub async fn test_start_cache_is_none() {
        let (tx, _) = mpsc::channel(1000);
        let cache = Arc::new(RwLock::new(None));
        let refresher: SymbolRefresher<MiniTicker> = SymbolRefresher::new(cache, "abc".to_string(), tx);
        let share_refresher = Arc::new(refresher);
        let cache_send = share_refresher.clone();
        tokio::spawn(async move {
            cache_send.start().await;
        });

        sleep(Duration::from_secs(2)).await;
        let cache = &*share_refresher.cache.read().await;
        assert!(cache.is_none());
    }


    #[tokio::test]
    pub async fn test_start() {
        let _ = setup_logger(Some(LevelFilter::Debug));
        let (tx, mut rx) = mpsc::channel(1000);
        let cache = Arc::new(RwLock::new(None));
        let refresher: SymbolRefresher<MiniTicker> = SymbolRefresher::new(cache.clone(), "abc".to_string(), tx);
        let cache_send = refresher.clone();
        tokio::spawn(async move {
            cache_send.start().await;
        });
        let data = MiniTicker {
            event_type: "bbc".to_string(),
            event_time: 1,
            symbol: "abc".to_string(),
            close: 0.1,
            open: 0.2,
            high: 0.3,
            low: 0.01,
            volume: 0.5,
            quote_volume: 0.6,
        };

        refresh_cache(&cache, data.clone()).await;

        sleep(Duration::from_secs(2)).await;

        let (ticker, _) = rx.recv().await.unwrap();
        assert_eq!(&ticker, &data, "数据更新不正确");
    }
}
