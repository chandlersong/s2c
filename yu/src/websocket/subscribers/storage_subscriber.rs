use crate::binance::models::po::KlinePo;
use crate::binance::models::SpotStreamTradeRecordPo;
use crate::config::{get_config, SpotWebSocketStreamConfig};
use crate::duck_db::DBProvider;
use crate::errors::YuError;
use actix::{Actor, Addr, AsyncContext, Context, Handler};
use duckdb::params;
use log::{debug, error, info};
use std::sync::OnceLock;
use std::time::Duration;
use yue::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamResponse;

pub(crate) static SPOT_STREAM_WRITER_ADDR: OnceLock<Addr<SpotStreamStorageActor>> = OnceLock::new();

pub fn get_spot_stream_writer() -> Addr<SpotStreamStorageActor> {
    SPOT_STREAM_WRITER_ADDR
        .get_or_init(|| SpotStreamStorageActor::start_new().unwrap())
        .clone()
}

/// 存储订阅者 Actor
/// 接收 Trade 和 Depth 事件，缓冲并批量写入 DuckDB
/// TODO:  定时删除旧信息
pub struct SpotStreamStorageActor {
    config: SpotWebSocketStreamConfig,
    db: DBProvider,
    trade_buffer: Vec<SpotStreamTradeRecordPo>,
    received_count: u64,
    flushed_trades: u64,
    flushed_depths: u64,
    kline_buffer: Vec<KlinePo>,
    flushed_klines: u64,
}

impl SpotStreamStorageActor {
    pub fn new(config: SpotWebSocketStreamConfig, db: DBProvider) -> Self {
        SpotStreamStorageActor {
            config,
            db,
            trade_buffer: Vec::new(),
            received_count: 0,
            flushed_trades: 0,
            flushed_depths: 0,
            kline_buffer: Vec::new(),
            flushed_klines: 0,
        }
    }

    pub fn start_new() -> Result<Addr<Self>, YuError> {
        let config = get_config();

        // 检查是否启用了 WebSocket 功能
        let ws_config = match &config.binance {
            Some(ws) => ws,
            None => {
                info!("binance_websocket 配置未启用，跳过 WebSocket 任务");
                return Err(YuError::new("binance_websocket 配置未启用，跳过 WebSocket 任务"));
            }
        };

        let spot_config = match &ws_config.spot_stream {
            Some(spot) => spot,
            None => {
                info!("binance_websocket.spot 配置未启用，跳过 Spot WebSocket 任务");
                return Err(YuError::new("binance_websocket 配置未启用，跳过 WebSocket 任务"));
            }
        };
        Ok(SpotStreamStorageActor::new(spot_config.clone(), DBProvider::default()).start())
    }

    fn get_trade_batch_size(&self) -> usize {
        self.config.trade.as_ref().and_then(|c| c.batch_size).unwrap_or(100)
    }

    fn get_flush_interval_ms(&self) -> u64 {
        // 使用 trade 的配置作为默认值
        self.config.trade.as_ref().and_then(|c| c.flush_interval_ms).unwrap_or(5000)
    }

    fn get_kline_batch_size(&self) -> usize {
        1000
    }

    fn get_kline_flush_interval_ms(&self) -> u64 {
        1000
    }

    fn flush_trades(&mut self) {
        if self.trade_buffer.is_empty() {
            return;
        }

        debug!("Flushing {} trade records to DuckDB", self.trade_buffer.len());

        match self.write_trades_to_db() {
            Ok(count) => {
                self.flushed_trades += count as u64;
                debug!("Successfully flushed {} trade records. Total flushed: {}", count, self.flushed_trades);
                self.trade_buffer.clear();
            }
            Err(e) => {
                error!("Failed to flush trade records: {:?}", e);
                self.trade_buffer.clear();
            }
        }
    }

    fn flush_klines(&mut self) {
        if self.kline_buffer.is_empty() {
            return;
        }

        debug!("Flushing {} kline records to DuckDB", self.kline_buffer.len());

        match self.write_klines_to_db() {
            Ok(count) => {
                self.flushed_klines += count as u64;
                debug!("Successfully flushed {} kline records. Total flushed: {}", count, self.flushed_klines);
                self.kline_buffer.clear();
            }
            Err(e) => {
                error!("Failed to flush kline records: {:?}", e);
                self.kline_buffer.clear();
            }
        }
    }

    fn write_trades_to_db(&self) -> Result<usize, crate::errors::YuError> {
        let conn = self.db.acquire()?;
        let mut appender = conn.appender("bn_spot_trade")?;

        for record in &self.trade_buffer {
            if let Err(e) = appender.append_row(params![
                record.id,
                record.event_time,
                &record.symbol,
                record.trade_id,
                record.price,
                record.qty,
                record.trade_time,
                record.is_buyer_maker,
                record.created_at,
            ]) {
                // FUTURE: 失败重试机制。
                // 先不考虑吧。因为应该会在check的时候，去补全

                error!("Failed to append trade records: {:?}", e);
            };
        }

        let _ = appender.flush();
        Ok(self.trade_buffer.len())
    }

    fn write_klines_to_db(&self) -> Result<usize, YuError> {
        let conn = self.db.acquire()?;
        let mut appender = conn.appender("bn_spot_kline")?;

        if let Err(e) = appender.flush() {
            error!("Failed to flush kline to db: {:?}", e);
        }
        Ok(self.kline_buffer.len())
    }

    fn buffer_kline(&mut self, kline: KlinePo) {
        self.kline_buffer.push(kline);
        if self.kline_buffer.len() >= self.get_kline_batch_size() {
            self.flush_klines();
        }
    }
}

impl Actor for SpotStreamStorageActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.set_mailbox_capacity(1000);
        info!("StorageSubscriberActor started with mailbox capacity: 10000");

        // 定时刷新交易
        let trade_interval = Duration::from_millis(self.get_flush_interval_ms());
        ctx.run_interval(trade_interval, |act, _ctx| {
            act.flush_trades();
        });

        // 定时刷新kline
        let kline_interval = Duration::from_millis(self.get_kline_flush_interval_ms());
        ctx.run_interval(kline_interval, |act, _ctx| {
            act.flush_klines();
        });
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("StorageSubscriberActor stopped, flushing remaining buffers");

        self.flush_trades();
        self.flush_klines();
        info!(
            "StorageSubscriberActor stopped. Total received: {}, Total flushed trades: {}, Total flushed depths: {}",
            self.received_count, self.flushed_trades, self.flushed_depths
        );
    }
}

/// 实现 Supervised trait 支持 Supervisor 启动
impl actix::Supervised for SpotStreamStorageActor {}

impl Handler<BinanceSpotWebSocketStreamResponse> for SpotStreamStorageActor {
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
            BinanceSpotWebSocketStreamResponse::Kline(kline) => {
                if kline.kline.is_closed == true {
                    let kline_po = KlinePo::from(kline.kline);
                    self.buffer_kline(kline_po);
                    if self.kline_buffer.len() >= self.get_kline_batch_size() {
                        self.flush_klines();
                    }
                }
            }
            _ => {
                // 目前只处理 Trade 消息，其他类型忽略
            }
        }
    }
}
