use crate::postgresql_db::CopyInsertable;
use async_trait::async_trait;
use sqlx_postgres::{PgPool, PgPoolCopyExt};
use std::sync::Arc;
use tokio::sync::mpsc;

///
/// 主要是为了高效率的批量写入
///

#[async_trait]
pub trait PostgresqlBatchInsertTrait<P: CopyInsertable>: Send + Sync {
    async fn insert_data(&self, data: P);
}

pub type PostgresqlBatchInsert<P> = Arc<dyn PostgresqlBatchInsertTrait<P>>;

pub struct PostgresqlBatchInsertImpl<P: CopyInsertable> {
    sender: mpsc::Sender<P>,
}

impl<P: CopyInsertable> PostgresqlBatchInsertImpl<P> {
    pub async fn new(table_name: &str, pg_pool: PgPool) -> PostgresqlBatchInsert<P> {
        let (sender, receiver) = mpsc::channel(1);
        let table = table_name.to_owned();
        tokio::spawn(async move {
            Self::start_at_backend(pg_pool, receiver, table, 1000, 1000).await;
        });
        Arc::new(Self { sender })
    }

    ///
    /// 接收rx传进来的P。然后在以下两个条件下，存入数据库
    /// 1. 缓存大于max_batch_size.
    /// 2. 上次刷新之后，超过flush_interval_ms
    /// 如果为空，则跳过。
    ///
    pub async fn start_at_backend(pg_pool: PgPool, mut rx: mpsc::Receiver<P>, table_name: String, max_batch_size: u64, flush_interval_ms: u64) {
        use tokio::time::{Duration, interval};

        let mut buffer: Vec<P> = Vec::with_capacity(max_batch_size as usize);
        let mut ticker = interval(Duration::from_millis(flush_interval_ms));

        loop {
            tokio::select! {
                maybe = rx.recv() => {
                    match maybe {
                        Some(item) => {
                            buffer.push(item);
                            if buffer.len() as u64 >= max_batch_size {
                                if let Err(e) = Self::copy_insert(&pg_pool, &buffer, &table_name).await {
                                    log::error!("batch copy_insert failed: {}", e);
                                }
                                buffer.clear();
                            }
                        }
                        None => {
                            // channel closed, flush remaining and exit
                            if !buffer.is_empty() {
                                if let Err(e) = Self::copy_insert(&pg_pool, &buffer, &table_name).await {
                                    log::error!("batch copy_insert failed: {}", e);
                                }
                                buffer.clear();
                            }
                            break;
                        }
                    }
                }
                _ = ticker.tick() => {
                    if !buffer.is_empty() {
                         if let Err(e) = Self::copy_insert(&pg_pool, &buffer, &table_name).await {
                            log::error!("batch copy_insert failed: {}", e);
                        }
                        buffer.clear();
                    }
                }
            }
        }
    }

    pub async fn copy_insert(pool: &PgPool, data: &[P], table_name: &str) -> Result<u64, sqlx::Error> {
        if data.is_empty() {
            return Ok(0);
        }

        let mut copy_in = pool
            .copy_in_raw(&format!(
                "COPY {} ({}) FROM STDIN WITH (FORMAT CSV, HEADER false)",
                table_name,
                P::columns()
            ))
            .await?;

        for item in data {
            let mut row = item.to_csv_row();
            if !row.ends_with('\n') {
                row.push('\n');
            }
            copy_in.send(row.as_bytes()).await?;
        }

        let rows = copy_in.finish().await?;
        Ok(rows)
    }
}

#[async_trait]
impl<P: CopyInsertable> PostgresqlBatchInsertTrait<P> for PostgresqlBatchInsertImpl<P> {
    async fn insert_data(&self, data: P) {
        if let Err(e) = self.sender.send(data).await {
            log::error!("Failed to send data to batch insert channel: {}", e);
        }
    }
}
