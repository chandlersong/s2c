use crate::tools::time::{current_date_string, get_next_utc_day_begin, instant_to_datetime};
use log::{error, info};
use rocksdb::{ColumnFamilyDescriptor, MultiThreaded, OptimisticTransactionDB, Options, DB};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{sleep_until, Instant};

pub struct RollingKVDBConfiguration {
    start: Instant,
    duration: Duration,
    path: PathBuf
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
        let start = get_next_utc_day_begin();
        let duration = Duration::from_secs(24 * 60 * 60);
        Self {
            start,
            duration,
            path
        }
    }
}

#[derive(Debug)]
pub struct RollingKvDBReport {
    record_count: u32, //存入多少数据
}

impl RollingKvDBReport {
    pub fn increment_record_count(&mut self, record_count: u32) {
        self.record_count += record_count;
    }

    pub fn record_count(&self) -> u32 {
        self.record_count
    }
}

impl Default for RollingKvDBReport {
    fn default() -> Self {
        Self { record_count: 0 }
    }
}

pub struct RollingKVDB {
    db: OptimisticTransactionDB<MultiThreaded>,
    current_cf: Arc<RwLock<String>>, // 持有 ColumnFamily 句柄
    close_refresh_cf_tx: mpsc::Sender<()>,
    report: Arc<Mutex<RollingKvDBReport>>
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
                    info!("收到关闭信号");
                    break;
                }
                // 循环任务
                _ = sleep_until(next_tick) => {
                    func();
                }
                _=tokio::signal::ctrl_c() => {
                    info!("程序主动关闭");
                    break;
                }
            }
        }
        error!("Loop task ended");
    });
}

impl RollingKVDB {
    pub async fn new(config: RollingKVDBConfiguration, cf_name: Option<String>) -> Self {

        let current_cf = match cf_name {
            Some(name) => Arc::new(RwLock::new(name)),
            None => Arc::new(RwLock::new(current_date_string())),
        };

        //open DB
        let mut options = Options::default();
        options.create_if_missing(true);
        options.create_missing_column_families(true);

        let existing_cfs = DB::list_cf(&options, &config.path).unwrap_or_else(|_| vec!["default".to_string()]);

        let mut cfs = Vec::new();
        for cf in &existing_cfs {
            cfs.push(ColumnFamilyDescriptor::new(cf, Options::default()));
        }
        let open_cf_arc = current_cf.clone();
        let open_cf = &open_cf_arc.read().unwrap();
        if !existing_cfs.contains(open_cf) {
            cfs.push(ColumnFamilyDescriptor::new(open_cf.to_string(), Options::default()));
        }

        let db = OptimisticTransactionDB::open_cf_descriptors(&options, config.path, cfs)
            .expect("Failed to open DB");

        // 刷新CF
        let cf = current_cf.clone();
        let (close_refresh_cf_tx, rx) = mpsc::channel::<()>(1);
        let swap_cf_func = move || {
            *cf.write().unwrap() = current_date_string();
        };

        loop_func(config.start, config.duration, swap_cf_func, rx).await;

        RollingKVDB { db, current_cf, close_refresh_cf_tx, report: Arc::new(Mutex::new(Default::default()))}
    }

    pub fn write_batch(&mut self, data: BatchData) -> Result<(), rocksdb::Error> {
        let current_cf_name = &self.current_cf.read().unwrap();
        let default_cf = match self.db.cf_handle(current_cf_name.as_str()) {
            Some(cf) => cf,
            None => {
                let result = self.current_cf.read().unwrap();
                let options = Options::default();
                self.db.create_cf(result.as_str(), &options).expect("TODO: panic message");
                self.db.cf_handle(result.as_str()).unwrap()
            }
        };
        let txn = self.db.transaction();
        for (key, value) in &data{
            txn.put_cf(&default_cf, key, value)?;
        }

        self.report.lock().unwrap().increment_record_count(data.len() as u32);
        txn.commit()
    }

    pub fn read_value(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        let result = self.current_cf.read().unwrap();
        let default_cf = self.db.cf_handle(result.as_str()).unwrap();
        match self.db.get_cf(&default_cf, key) {
            Ok(Some(value)) => Some(value.to_vec()),
            Ok(None) => { None }
            Err(e) => {
                error!("Error reading value from DB: key is {},error is {}",String::from_utf8_lossy(key),e);
                None
            }
        }
    }

    pub async fn close(&self) {
        info!("Closing RollingKVDB");
        self.close_refresh_cf_tx.send(()).await.expect("关闭cf刷新失败");
    }

    pub async fn get_report(&self) -> Arc<Mutex<RollingKvDBReport>> {
        self.report.clone()
    }
}

#[cfg(test)]
mod tests {
    use crate::database::rolling_kv_db::{loop_func, RollingKVDB, RollingKVDBConfiguration};
    use crate::tools::time::instant_to_datetime;
    use chrono::{Datelike, Timelike};
    use rocksdb::{Options, DB};
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tokio::time::{sleep, Instant};

    impl RollingKVDBConfiguration {
        pub fn new(mill_seconds: u64, path: &str) -> Self {
            Self {
                start: Instant::now() + Duration::from_millis(50),
                duration: Duration::from_millis(mill_seconds),
                path: PathBuf::from(path)
            }
        }
    }

    const DATA_FOLDER: &str = "./tests/db/rolling_db";

    /// 做CF切换的时候，需要能够存入
    #[tokio::test(flavor = "multi_thread")]
    async fn test_create_cf_change() {
        let db_folder = &format!("{}/cf_change",DATA_FOLDER);
        let config = RollingKVDBConfiguration::new(100, db_folder);
        let options = Options::default();
        if Path::new(db_folder).exists() {
            DB::destroy(&options, db_folder).unwrap();
        }
        fs::create_dir_all(&config.path).unwrap();
        println!("path: {:?}", config.path.canonicalize());
        let mut rolling_db = RollingKVDB::new(config, Some("test".to_string())).await;
        let prev_cf = rolling_db.current_cf.read().unwrap().clone();
        sleep(Duration::from_millis(500)).await;
        let actual_cf = rolling_db.current_cf.read().unwrap().clone();
        println!("new cf: {:?}", actual_cf);
        assert_ne!(prev_cf, actual_cf);


        let mut data = HashMap::new();
        data.insert(b"key".to_vec(), b"value".to_vec());

        rolling_db.write_batch(data).unwrap();

        let value = rolling_db.read_value(&b"key".to_vec()).unwrap();
        assert_eq!(value, b"value");


        rolling_db.close().await;
    }

    /// 如果一个新的数据库。没有任何CF。应该能够完成处理
    #[tokio::test(flavor = "multi_thread")]
    async fn test_with_new_cf() {
        let db_folder = &format!("{}/new_cf",DATA_FOLDER);
        let config = RollingKVDBConfiguration::new(60 * 1000, db_folder);
        let _options = Options::default();
        if Path::new(db_folder).exists() {
            DB::destroy(&_options, db_folder).unwrap();
        }
        fs::create_dir_all(&config.path).unwrap();
        println!("path: {:?}", config.path.canonicalize());
        let mut rolling_db = RollingKVDB::new(config, Some("test".to_string())).await;

        let mut data = HashMap::new();
        data.insert(b"key".to_vec(), b"value".to_vec());

        rolling_db.write_batch(data).unwrap();

        let value = rolling_db.read_value(&b"key".to_vec()).unwrap();
        assert_eq!(value, b"value");
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
