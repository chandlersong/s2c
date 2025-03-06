use crate::tools::time::{current_date_string, instant_to_datetime};
use chrono::{Datelike, Duration as ChronoDuration, TimeZone, Utc};
use log::{error, info, trace};
use rocksdb::{OptimisticTransactionDB, Options};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{sleep_until, Instant};

pub struct RollingKVDBConfiguration {
    start: Instant,
    duration: Duration,
    path: PathBuf,
}

impl fmt::Display for RollingKVDBConfiguration {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "rolling db configuration: path: {:?}, start time : {:?}, duration: {:?} }}",
               self.path,
               instant_to_datetime(self.start),
               self.duration)
    }
}


pub type BatchData = HashMap<Vec<u8>, Vec<u8>>;

impl RollingKVDBConfiguration {
    pub fn new_with_path(path: PathBuf) -> Self {
        let now = Utc::now();
        // 计算今天的 00:00 UTC
        let today_midnight = Utc
            .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
            .single()
            .expect("Failed to create UTC midnight");

        // 计算今天的 24:00（即下一天的 00:00）
        let today_end = today_midnight + ChronoDuration::days(1);

        // 使用 UNIX_EPOCH 作为基准
        let unix_epoch = chrono::DateTime::<Utc>::UNIX_EPOCH;

        // 计算时间差
        let now_duration = now - unix_epoch;
        let today_end_duration = today_end - unix_epoch;

        // 转换为 Instant
        let instant_now = Instant::now();
        let start = instant_now + (today_end_duration - now_duration).to_std().expect("Duration out of range");
        let duration = Duration::from_secs(24 * 60 * 60);

        Self {
            start,
            duration,
            path,
        }
    }
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
                    info!("Closing loop");
                    break;
                }
                // 循环任务
                _ = sleep_until(next_tick) => {
                    trace!("Sleeping until next tick");
                    func();
                }
            }
        }
        error!("Loop task ended");
    });
}

impl RollingKVDB {
    pub async fn new(config: RollingKVDBConfiguration, cf_name: Option<String>) -> Self {
        let mut options = Options::default();
        options.create_if_missing(true);
        let db = OptimisticTransactionDB::open(&options, config.path).unwrap();
        let current_cf = match cf_name {
            Some(name) => Arc::new(RwLock::new(name)),
            None => Arc::new(RwLock::new(current_date_string())),
        };
        let cf = current_cf.clone();
        let (tx, rx) = mpsc::channel::<()>(1);

        let swap_cf_func = move || {
            *cf.write().unwrap() = current_date_string();
        };

        loop_func(config.start, config.duration, swap_cf_func, rx).await;

        RollingKVDB { db, current_cf, close_refresh_cf_tx: tx }
    }

    pub fn write_batch(&self, data:BatchData) -> Result<(), rocksdb::Error> {
        let txn = self.db.transaction();
        let result = self.current_cf.read().unwrap();
        let default_cf = self.db.cf_handle(result.as_str()).unwrap();
        for (key, value) in &data{
            txn.put_cf(default_cf, key, value)?;
        }
        txn.commit()
    }

    pub async fn close(&self) {
        info!("Closing RollingKVDB");
        self.close_refresh_cf_tx.send(()).await.expect("TODO: panic message");
    }
}

#[cfg(test)]
mod tests {
    use crate::database::rolling_kv_db::{loop_func, RollingKVDB, RollingKVDBConfiguration};
    use crate::tools::time::instant_to_datetime;
    use chrono::{Datelike, Timelike};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tokio::time::{sleep, Instant};

    impl RollingKVDBConfiguration {
        pub fn new(mill_seconds: u64, path: &str) -> Self {
            Self {
                start: Instant::now() + Duration::from_millis(50),
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
        let rolling_db = RollingKVDB::new(config, Some("test".to_string())).await;
        let prev_cf = rolling_db.current_cf.read().unwrap().clone();
        sleep(Duration::from_millis(500)).await;
        let actual_cf = rolling_db.current_cf.read().unwrap().clone();
        println!("new cf: {:?}", actual_cf);
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

    #[test]
    fn test_default_configuration() {
        let config = RollingKVDBConfiguration::new_with_path(PathBuf::new());
        let target_datetime = instant_to_datetime(config.start);
        let year = target_datetime.year();
        let month = target_datetime.month();
        let day = target_datetime.day();
        let hour = target_datetime.hour();
        let minute = target_datetime.minute();
        let second = target_datetime.second();
        println!("UTC DateTime: {}", target_datetime);
        println!("Year: {}, Month: {}, Day: {}, hour: {} ,minute: {}, second:   {}", year, month, day, hour, minute, second);
        assert_eq!(hour, 0);
        assert_eq!(minute, 0);
        assert_eq!(second, 0);
    }
    
}
