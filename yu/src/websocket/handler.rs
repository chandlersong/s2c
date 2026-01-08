use actix::{Actor, Addr, Context, Handler, Message, Recipient};
use log::{error, info};
use std::sync::Arc;

use crate::binance::models::{DepthRecordPo, TradeRecordPo};
use crate::config::SpotWebSocketStreamConfig;
use yue::websocket::client::{SendTextMessage, SubscribeToEvents, WebSocketClient, WebSocketEvent};
use crate::errors::YuError;
use crate::websocket::binance_spot::SpotMessageParser;
use yue::binance::bn_json_websocket::{SPOT_STREAM_WEBSOCKET, WS_SUBSCRIBE_COMMAND, StreamCommandRequest};

/// 简单缓冲，内存保存，按需刷新（目前不持久化）
pub struct StorageBuffer {
    pub trade_buffer: Vec<TradeRecordPo>,
    pub depth_buffer: Vec<DepthRecordPo>,
    pub batch_size: usize,
}

impl StorageBuffer {
    pub fn new(batch_size: usize) -> Self {
        StorageBuffer {
            trade_buffer: Vec::with_capacity(batch_size),
            depth_buffer: Vec::with_capacity(batch_size),
            batch_size,
        }
    }

    pub fn add_trade(&mut self, r: TradeRecordPo) {
        self.trade_buffer.push(r);
    }

    pub fn add_depth(&mut self, r: DepthRecordPo) {
        self.depth_buffer.push(r);
    }

    pub fn flush_all(&mut self) {
        // TODO: 持久化到 DuckDB，目前仅清空缓冲
        self.trade_buffer.clear();
        self.depth_buffer.clear();
    }
}

/// 用于查询状态的消息（测试/监控用）
pub struct GetStats;
impl Message for GetStats {
    type Result = (usize, usize, u64);
}

pub struct BinanceWebSocketDataCollector {
    pub config: SpotWebSocketStreamConfig,
    pub storage_buffer: StorageBuffer,
    pub ws_client: Option<Addr<WebSocketClient>>,
    pub message_count: u64,
}

impl BinanceWebSocketDataCollector {
    pub fn new(config: SpotWebSocketStreamConfig) -> Self {
        let batch = config.trade.as_ref().and_then(|c| c.batch_size).unwrap_or(100);
        BinanceWebSocketDataCollector {
            config,
            storage_buffer: StorageBuffer::new(batch),
            ws_client: None,
            message_count: 0,
        }
    }

    fn build_params_from_stream(&self, cfg: &Option<crate::config::StreamConfig>, suffix: &str) -> Vec<String> {
        let mut params = Vec::new();
        if let Some(ref c) = cfg {
            if c.enabled.unwrap_or(true) {
                params.extend(c.symbols.iter().map(|s| format!("{}{}", s.to_lowercase(), suffix)));
            }
        }
        params
    }

    fn subscribe_streams(&self, client: &Addr<WebSocketClient>) {
        // 使用 build_subscribe_request 生成订阅请求并发送
        if let Some(subscribe) = self.build_subscribe_request(1) {
            let text = match serde_json::to_string(&subscribe) {
                Ok(t) => t,
                Err(e) => {
                    error!("failed to serialize subscribe request: {}", e);
                    return;
                }
            };
            let msg = SendTextMessage { text };
            let _ = client.try_send(msg);
        } else {
            info!("没有配置要订阅的 streams，跳过订阅");
        }
    }

    fn build_subscribe_request(&self, id: u64) -> Option<StreamCommandRequest> {
        let mut params: Vec<String> = Vec::new();
        params.extend(self.build_params_from_stream(&self.config.trade, "@trade"));
        params.extend(self.build_params_from_stream(&self.config.depth_update, "@depth"));
        if params.is_empty() {
            return None;
        }
        Some(StreamCommandRequest { method: WS_SUBSCRIBE_COMMAND.to_string(), params, id })
    }

    fn handle_event(&mut self, event: WebSocketEvent) {
        match event {
            WebSocketEvent::Connected => {
                info!("WebSocket connected, subscribing streams");
                if let Some(ref client) = self.ws_client {
                    self.subscribe_streams(client);
                }
            }
            WebSocketEvent::Reconnecting => {
                info!("WebSocket reconnecting, will resubscribe on connected");
            }
            WebSocketEvent::TextMessage(text) => {
                // 解析并缓冲
                match SpotMessageParser::parse_and_route(&text) {
                    Ok(Some(yue_collected)) => {
                        match yue_collected {
                            crate::websocket::binance_spot::spot_parser::ParseResult::Trade(t) => {
                                self.storage_buffer.add_trade(t);
                                self.message_count += 1;
                                if self.storage_buffer.trade_buffer.len() >= self.storage_buffer.batch_size {
                                    self.storage_buffer.flush_all();
                                }
                            }
                            crate::websocket::binance_spot::spot_parser::ParseResult::Depth(d) => {
                                self.storage_buffer.add_depth(d);
                                self.message_count += 1;
                                if self.storage_buffer.depth_buffer.len() >= self.storage_buffer.batch_size {
                                    self.storage_buffer.flush_all();
                                }
                            }
                        }
                    }
                    Ok(None) => { /* ignore other events */ }
                    Err(e) => {
                        error!("parser error: {:?}", e);
                    }
                }
            }
            WebSocketEvent::Disconnected => {
                info!("WebSocket disconnected, flushing buffer");
                self.storage_buffer.flush_all();
            }
            WebSocketEvent::Error(err) => {
                error!("WebSocket error: {}", err);
            }
            WebSocketEvent::BinaryMessage(_) => {
                // ignore binary
            }
        }
    }
}

impl Actor for BinanceWebSocketDataCollector {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("BinanceWebSocketDataCollector actor started");

        // 启动并订阅 WebSocketClient
        // 使用默认 Spot 流地址
        let client = WebSocketClient::new(SPOT_STREAM_WEBSOCKET).start();
        // 订阅事件
        let recipient: Recipient<WebSocketEvent> = ctx.address().recipient();
        let _ = client.do_send(SubscribeToEvents { recipient });
        self.ws_client = Some(client);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("BinanceWebSocketDataCollector stopped, flushing buffer");
        self.storage_buffer.flush_all();
    }
}

impl Handler<GetStats> for BinanceWebSocketDataCollector {
    type Result = (usize, usize, u64);

    fn handle(&mut self, _msg: GetStats, _ctx: &mut Context<Self>) -> Self::Result {
        (
            self.storage_buffer.trade_buffer.len(),
            self.storage_buffer.depth_buffer.len(),
            self.message_count,
        )
    }
}

impl Handler<WebSocketEvent> for BinanceWebSocketDataCollector {
    type Result = ();

    fn handle(&mut self, event: WebSocketEvent, _ctx: &mut Context<Self>) -> Self::Result {
        self.handle_event(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{StreamConfig, SpotWebSocketStreamConfig};

    fn make_stream_config(enabled: Option<bool>, symbols: Vec<&str>) -> StreamConfig {
        StreamConfig {
            enabled,
            symbols: symbols.into_iter().map(|s| s.to_string()).collect(),
            batch_size: None,
            flush_interval_ms: None,
            retention_days: None,
        }
    }

    #[test]
    fn test_build_params_from_stream_trade() {
        let trade_cfg = Some(make_stream_config(Some(true), vec!["BTCUSDT", "ETHUSDT"]));
        let cfg = SpotWebSocketStreamConfig { trade: trade_cfg.clone(), depth_update: None };
        let collector = BinanceWebSocketDataCollector::new(cfg);

        let params = collector.build_params_from_stream(&trade_cfg, "@trade");
        assert_eq!(params.len(), 2);
        assert!(params.contains(&"btcusdt@trade".to_string()));
        assert!(params.contains(&"ethusdt@trade".to_string()));
    }

    #[test]
    fn test_build_params_from_stream_depth_disabled() {
        let depth_cfg = Some(make_stream_config(Some(false), vec!["BTCUSDT"]));
        let cfg = SpotWebSocketStreamConfig { trade: None, depth_update: depth_cfg.clone() };
        let collector = BinanceWebSocketDataCollector::new(cfg);

        let params = collector.build_params_from_stream(&depth_cfg, "@depth");
        assert!(params.is_empty());
    }

    #[test]
    fn test_build_params_from_stream_empty_symbols() {
        let cfg_empty = Some(make_stream_config(Some(true), vec![]));
        let cfg = SpotWebSocketStreamConfig { trade: cfg_empty.clone(), depth_update: None };
        let collector = BinanceWebSocketDataCollector::new(cfg);

        let params = collector.build_params_from_stream(&cfg_empty, "@trade");
        assert!(params.is_empty());
    }

    #[test]
    fn test_build_subscribe_request() {
        let trade_cfg = Some(make_stream_config(Some(true), vec!["BTCUSDT"]));
        let depth_cfg = Some(make_stream_config(Some(true), vec!["ETHUSDT"]));
        let cfg = SpotWebSocketStreamConfig { trade: trade_cfg.clone(), depth_update: depth_cfg.clone() };
        let collector = BinanceWebSocketDataCollector::new(cfg);

        let req = collector.build_subscribe_request(5).expect("should build request");
        assert_eq!(req.method, WS_SUBSCRIBE_COMMAND.to_string());
        assert_eq!(req.id, 5);
        assert!(req.params.contains(&"btcusdt@trade".to_string()));
        assert!(req.params.contains(&"ethusdt@depth".to_string()));
    }

    #[test]
    fn test_handle_event_text_trade_adds_to_buffer() {
        let trade_cfg = Some(make_stream_config(Some(true), vec!["BTCUSDT"]));
        let cfg = SpotWebSocketStreamConfig { trade: trade_cfg.clone(), depth_update: None };
        let mut collector = BinanceWebSocketDataCollector::new(cfg);

        // initial counts
        assert_eq!(collector.storage_buffer.trade_buffer.len(), 0);
        assert_eq!(collector.message_count, 0);

        let text = r#"{"e":"trade","E":1620000000000,"s":"BTCUSDT","t":12345,"p":"34123.12","q":"0.001","b":111,"a":222,"T":1620000001000,"m":false}"#;
        collector.handle_event(WebSocketEvent::TextMessage(text.to_string()));

        assert_eq!(collector.storage_buffer.trade_buffer.len(), 1);
        assert_eq!(collector.message_count, 1);
    }

    #[test]
    fn test_handle_event_disconnected_flushes() {
        let trade_cfg = Some(make_stream_config(Some(true), vec!["BTCUSDT"]));
        let cfg = SpotWebSocketStreamConfig { trade: trade_cfg.clone(), depth_update: None };
        let mut collector = BinanceWebSocketDataCollector::new(cfg);

        // add dummy trade
        let po = TradeRecordPo {
            event_time: 1,
            symbol: "BTCUSDT".to_string(),
            trade_id: 1,
            price: 1.0,
            qty: 1.0,
            buyer_order_id: None,
            seller_order_id: None,
            trade_time: None,
            is_buyer_maker: None,
            created_at: 0,
        };
        collector.storage_buffer.add_trade(po);
        assert_eq!(collector.storage_buffer.trade_buffer.len(), 1);

        collector.handle_event(WebSocketEvent::Disconnected);
        assert_eq!(collector.storage_buffer.trade_buffer.len(), 0);
    }
}
