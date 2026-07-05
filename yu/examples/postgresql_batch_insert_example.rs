use li::tools::logs::setup_logger;
use log::{LevelFilter, error, info};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use yu::config::get_config;
use yu::errors::YuError;
use yu::postgresql_db::get_sync_client_pg_pool;
use yu::postgresql_db_tables::PostgresqlBatchInsertImpl;
use yu::sync::client::po::LocalPolyMarketHistoryPo;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let _app_config = get_config();
    let mut special_log = HashMap::new();
    special_log.insert("li".to_string(), LevelFilter::Trace);
    special_log.insert("yu".to_string(), LevelFilter::Trace);
    special_log.insert("postgresql_batch_insert_example".to_string(), LevelFilter::Trace);
    special_log.insert("yue".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Warn), special_log).unwrap();

    let pg_pool = get_sync_client_pg_pool().await?;
    info!("创建测试的表");
    let table_name = "batch_insert_example";
    info!("准备执行动态建表 SQL，已校验表名: {}", table_name);

    // 使用与 LocalPolyMarketHistoryPo 对应的列类型（bigint 而非 timestamptz）
    let create_table_sql = "CREATE TABLE IF NOT EXISTS batch_insert_example (
            id bigint,
            asset_id TEXT,
            timestamp timestamptz,
            price DOUBLE PRECISION,
            batch_timestamp timestamptz
        );";
    if let Err(e) = sqlx::query(create_table_sql).execute(&pg_pool).await {
        error!("创建数据库表失败:{}", e);
    }
    let batch_insert = PostgresqlBatchInsertImpl::<LocalPolyMarketHistoryPo>::new(table_name, pg_pool.clone()).await;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    for i in 0..1000u64 {
        let item = LocalPolyMarketHistoryPo {
            id: i + 1,
            asset_id: format!("series_{}", i + 1),
            timestamp: now,
            price: 100.0 + ((i % 100) as f64) * 0.123, // 简单变化，避免全相同
            batch_timestamp: now,
        };
        batch_insert.insert_data(item).await;
    }

    info!("已插入 1000 条记录，等待 10 秒钟...");
    sleep(Duration::from_secs(10)).await;

    let pos: Vec<LocalPolyMarketHistoryPo> = sqlx::query_as("select * from batch_insert_example").fetch_all(&pg_pool).await?;
    info!("查询到 {} 条记录", pos.len());
    let first_po = pos.first().unwrap();
    assert_eq!(first_po.batch_timestamp, now);
    Ok(())
}
