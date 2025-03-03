use rocksdb::{OptimisticTransactionDB, Options};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{sleep, sleep_until, Instant};

pub struct RollingKVDBConfiguration {
    duration: Duration,
    path: PathBuf,
}


pub struct RollingKVDB {
    db: OptimisticTransactionDB,
    current_cf: Arc<RwLock<String>>, // 持有 ColumnFamily 句柄
    close_refresh_cf_tx: mpsc::Sender<()>,
}


async fn loop_func<F>(start: Instant, duration: Duration, func: F, mut close_refresh_cf_rx: mpsc::Receiver<()>)
where
    F: Fn() + Send + 'static,
{
    tokio::spawn(async move {
        let mut next_tick = start;
        loop {
            next_tick = next_tick + duration;
            tokio::select! {
                // 接收停止信号
                _ = close_refresh_cf_rx.recv() => {
                    break;
                }
                // 循环任务
                _ = sleep_until(next_tick) => {
                    println!("Sleeping until next tick");
                    func();
                }
            }
        }

        println!("Loop task ended");
    });
}
impl RollingKVDB {
    pub async fn new(config: RollingKVDBConfiguration) -> Self {
        let mut options = Options::default();
        options.create_if_missing(true);
        let db = OptimisticTransactionDB::open(&options, config.path).unwrap();
        let current_cf = Arc::new(RwLock::new(String::from("test")));
        let cf = current_cf.clone();
        let (tx, mut rx) = mpsc::channel::<()>(1);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                // 接收停止信号
                _ = rx.recv() => {
                    println!("Received stop signal, stop refresh column familly");
                    break;
                }
                // 循环任务
                _ = sleep(config.duration) => {
                   *cf.write().unwrap() = String::from("test111");
                }
            }
            }

            println!("Loop task ended");
        });
        RollingKVDB { db, current_cf, close_refresh_cf_tx: tx }
    }

    pub fn write(&self, key: &[u8], value: &[u8]) -> Result<(), rocksdb::Error> {
        let txn = self.db.transaction();
        let result = self.current_cf.read().unwrap();
        let default_cf = self.db.cf_handle(result.as_str()).unwrap();
        txn.put_cf(default_cf, key, value)?;
        txn.commit()
    }

    pub async fn close(&self) {
        self.close_refresh_cf_tx.send(()).await.expect("TODO: panic message");
    }
}

#[cfg(test)]
mod tests {
    use crate::database::rolling_kv_db::{loop_func, RollingKVDB, RollingKVDBConfiguration};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tokio::time::{sleep, Instant};

    impl RollingKVDBConfiguration {
        pub fn new(mill_seconds: u64, path: &str) -> Self {
            Self {
                duration: Duration::from_millis(mill_seconds),
                path: PathBuf::from(path),
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_create_db() {
        let config = RollingKVDBConfiguration::new(100, "tests/db/rolling_db");
        if config.path.exists() {
            fs::remove_dir_all(&config.path).unwrap();
        }
        fs::create_dir_all(&config.path).unwrap();
        println!("path: {:?}", config.path.canonicalize());
        let rolling_db = RollingKVDB::new(config).await;
        let prev_cf = rolling_db.current_cf.read().unwrap().clone();
        sleep(Duration::from_millis(500)).await;
        let actual_cf = rolling_db.current_cf.read().unwrap().clone();
        assert_ne!(prev_cf, actual_cf);
        rolling_db.close().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_loop_run() {
        let start = Instant::now() + Duration::from_millis(100);
        let duration = Duration::from_millis(100);
        let data = Arc::new(Mutex::new("ok".to_string()));
        let data_change = data.clone();
        let func = move || {
            *data_change.lock().unwrap() = "change".to_string();
        };
        let (tx, rx) = mpsc::channel(1);
        loop_func(start, duration, func, rx).await;
        sleep(Duration::from_millis(300)).await;
        tx.send(()).await.unwrap();

        assert_eq!(data.lock().unwrap().as_str(), "change");
    }
}
