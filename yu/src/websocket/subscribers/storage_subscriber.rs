use crate::binance::models::SpotStreamTradeRecordPo;
use crate::config::SpotWebSocketStreamConfig;
use crate::duck_db::DBProvider;
use actix::{Actor, AsyncContext, Context, Handler};
use duckdb::params;
use log::{debug, error, info};
use std::time::Duration;
use yue::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamResponse;

/// 存储订阅者 Actor
/// 接收 Trade 和 Depth 事件，缓冲并批量写入 DuckDB
pub struct StorageSubscriberActor {
    config: SpotWebSocketStreamConfig,
    db: DBProvider,
    trade_buffer: Vec<SpotStreamTradeRecordPo>,
    received_count: u64,
    flushed_trades: u64,
    flushed_depths: u64,
}

impl StorageSubscriberActor {
    pub fn new(config: SpotWebSocketStreamConfig, db: DBProvider) -> Self {
        StorageSubscriberActor {
            config,
            db,
            trade_buffer: Vec::new(),
            received_count: 0,
            flushed_trades: 0,
            flushed_depths: 0,
        }
    }

    fn get_trade_batch_size(&self) -> usize {
        self.config.trade.as_ref().and_then(|c| c.batch_size).unwrap_or(100)
    }

    fn get_flush_interval_ms(&self) -> u64 {
        // 使用 trade 的配置作为默认值
        self.config.trade.as_ref().and_then(|c| c.flush_interval_ms).unwrap_or(5000)
    }

    fn flush_trades(&mut self) {
        if self.trade_buffer.is_empty() {
            return;
        }

        debug!("Flushing {} trade records to DuckDB", self.trade_buffer.len());

        match self.write_trades_to_db() {
            Ok(count) => {
                self.flushed_trades += count as u64;
                info!("Successfully flushed {} trade records. Total flushed: {}", count, self.flushed_trades);
                self.trade_buffer.clear();
            }
            Err(e) => {
                error!("Failed to flush trade records: {:?}", e);
                // 错误时也清空缓冲区，避免内存无限增长
                self.trade_buffer.clear();
            }
        }
    }

    fn write_trades_to_db(&self) -> Result<usize, crate::errors::YuError> {
        let conn = self.db.acquire()?;
        let mut appender = conn.appender("bn_spot_trade")?;

        for record in &self.trade_buffer {
            appender.append_row(params![
                record.event_time,
                &record.symbol,
                record.trade_id,
                record.price,
                record.qty,
                record.trade_time,
                record.is_buyer_maker,
                record.created_at,
            ])?;
        }

        let _ = appender.flush();
        Ok(self.trade_buffer.len())
    }
}

impl Actor for StorageSubscriberActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("StorageSubscriberActor started");

        // 定时刷新缓冲区
        let flush_interval = Duration::from_millis(self.get_flush_interval_ms());
        ctx.run_interval(flush_interval, |act, _ctx| {
            act.flush_trades();
        });
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("StorageSubscriberActor stopped, flushing remaining buffers");

        // 停止时刷新所有缓冲
        self.flush_trades();
        info!(
            "StorageSubscriberActor stopped. Total received: {}, Total flushed trades: {}, Total flushed depths: {}",
            self.received_count, self.flushed_trades, self.flushed_depths
        );
    }
}

/// 实现 Supervised trait 支持 Supervisor 启动
impl actix::Supervised for StorageSubscriberActor {}

impl Handler<BinanceSpotWebSocketStreamResponse> for StorageSubscriberActor {
    type Result = ();

    fn handle(&mut self, msg: BinanceSpotWebSocketStreamResponse, _ctx: &mut Context<Self>) -> Self::Result {
        self.received_count += 1;

        match msg {
            BinanceSpotWebSocketStreamResponse::Trade(trade) => {
                // 将 String 价格和数量转为 f64

                self.trade_buffer.push(SpotStreamTradeRecordPo::from(trade));
                debug!("Buffered trade, buffer size: {}", self.trade_buffer.len());

                // 检查是否达到批量大小
                if self.trade_buffer.len() >= self.get_trade_batch_size() {
                    self.flush_trades();
                }
            }
            _ => {
                // 目前只处理 Trade 消息，其他类型忽略
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StreamConfig;

    #[test]
    fn test_get_trade_batch_size() {
        let config = SpotWebSocketStreamConfig {
            trade: Some(StreamConfig {
                enabled: Some(true),
                symbols: vec!["BTCUSDT".to_string()],
                batch_size: Some(200),
                flush_interval_ms: Some(5000),
                retention_days: Some(7),
            }),
            depth_update: None,
        };

        assert_eq!(config.trade_batch_size(), 200);
    }

    #[test]
    fn test_get_depth_batch_size() {
        let config = SpotWebSocketStreamConfig {
            trade: None,
            depth_update: Some(StreamConfig {
                enabled: Some(true),
                symbols: vec!["BTCUSDT".to_string()],
                batch_size: Some(150),
                flush_interval_ms: Some(3000),
                retention_days: Some(3),
            }),
        };

        assert_eq!(config.depth_batch_size(), 150);
    }

    #[test]
    fn test_default_batch_sizes() {
        let config = SpotWebSocketStreamConfig {
            trade: None,
            depth_update: None,
        };

        assert_eq!(config.trade_batch_size(), 100);
        assert_eq!(config.depth_batch_size(), 50);
    }
}
