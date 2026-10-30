use crate::binance::bn_backend_service::get_spot_trading_table;
use crate::binance::bn_dashboard::{BinanceDashboard, InstrumentProvider, SpotInstrumentProvider, SwapInstrumentProvider};
use crate::binance::models::po::{BinanceInstrument, KlinePo, SpotStreamTradeRecordPo};
use crate::duck_db_tables::DuckTableTableChannel;
use crate::errors::YuError;
use crate::sync::models::grpc_sync::HistoryEntry;
use async_trait::async_trait;
use governor::Jitter;
use li::websocket::connection::{
    CommandMessage, ConnectionAction, MessageHandlerTrait, ShareMessageHandler, WebSocketConnection, WebSocketInterface,
};
use li::websocket::models::WebSocketMessage;
use log::{debug, error, info};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;
use yue::binance::bn_json_websocket::{SPOT_STREAM_WEBSOCKET, SWAP_MARKET_STREAM_WEBSOCKET};
use yue::binance::bn_models::spot_websocket_stream::{
    BinanceSpotWebSocketStreamResponse, BinanceSpotWebSocketStreamResponse::Kline, BinanceSpotWebSocketStreamWrapper,
};
use yue::binance::bn_models::swap_websocket_stream::{BinanceSwapWebSocketStreamResponse, BinanceSwapWebSocketStreamWrapper};
use yue::models::HistoryInterval;
use yue::query_message::{InsertPayload, QueryCommand};

const DEFAULT_RECONNECT_INTERVAL: Duration = Duration::from_secs(90 * 60);

///
/// 这个服务，主要后段，负责和websocket通行的一些service
///
///

struct SpotKlineSaver {
    db: DuckTableTableChannel<KlinePo>,
    instrument_provider: InstrumentProvider,
    sync_broadcast: mpsc::Sender<HistoryEntry>,
}

impl SpotKlineSaver {
    fn new_for_sync(
        db: DuckTableTableChannel<KlinePo>,
        instrument_provider: InstrumentProvider,
        history_tx: mpsc::Sender<HistoryEntry>,
    ) -> ShareMessageHandler<BinanceSpotWebSocketStreamWrapper> {
        Arc::new(SpotKlineSaver {
            db,
            instrument_provider,
            sync_broadcast: history_tx,
        })
    }
}
#[async_trait]
impl MessageHandlerTrait<BinanceSpotWebSocketStreamWrapper> for SpotKlineSaver {
    async fn handle_message(&self, message: &BinanceSpotWebSocketStreamWrapper) {
        match &message.data {
            Kline(payload) => {
                if payload.kline.is_closed {
                    let Some(instrument) = self.instrument_provider.get_instrument(&payload.kline.symbol) else {
                        error!("cannot save Binance Spot kline for unknown symbol {}", payload.kline.symbol);
                        return;
                    };
                    let inst_id = instrument.id as u64;
                    let kline = KlinePo::from_kline_stream_payload(payload.clone(), inst_id);
                    if let Err(e) = self.db.send(QueryCommand::Insert(InsertPayload::new_no_replay(kline.clone()))).await {
                        error!("Error save spot kline: {}", e);
                    }
                    if let Err(error) = self.sync_broadcast.send(kline.to_spot_history_entry()).await {
                        error!("failed to broadcast live Binance Spot kline {}: {}", payload.kline.symbol, error);
                    }
                }
            }
            _ => {
                //ignore other message types
            }
        }
    }
}

pub struct SpotTradeSaver {
    db: DuckTableTableChannel<SpotStreamTradeRecordPo>,
    instrument_provider: InstrumentProvider,
    sync_broadcast: mpsc::Sender<HistoryEntry>,
}

impl SpotTradeSaver {
    pub fn new(
        db: DuckTableTableChannel<SpotStreamTradeRecordPo>,
        instrument_provider: InstrumentProvider,
        sync_broadcast: mpsc::Sender<HistoryEntry>,
    ) -> ShareMessageHandler<BinanceSpotWebSocketStreamWrapper> {
        Arc::new(SpotTradeSaver {
            db,
            instrument_provider,
            sync_broadcast,
        })
    }

    pub async fn for_production(
        dashboard: Arc<BinanceDashboard>,
        sync_broadcast: mpsc::Sender<HistoryEntry>,
    ) -> ShareMessageHandler<BinanceSpotWebSocketStreamWrapper> {
        let instrument_provider = SpotInstrumentProvider::new_provider(dashboard);
        let db = get_spot_trading_table();
        Self::new(db, instrument_provider, sync_broadcast)
    }
}

#[async_trait]
impl MessageHandlerTrait<BinanceSpotWebSocketStreamWrapper> for SpotTradeSaver {
    async fn handle_message(&self, message: &BinanceSpotWebSocketStreamWrapper) {
        if let BinanceSpotWebSocketStreamResponse::Trade(payload) = &message.data {
            let Some(instrument) = self.instrument_provider.get_instrument(&payload.symbol) else {
                error!("cannot save Binance Spot trade for unknown symbol {}", payload.symbol);
                return;
            };
            let trade = SpotStreamTradeRecordPo::from_trade_stream_payload(payload.clone(), instrument.id as u64);
            if let Err(error) = self.db.send(QueryCommand::Insert(InsertPayload::new_no_replay(trade.clone()))).await {
                error!("Error save spot trade: {}", error);
            }
            if let Err(error) = self.sync_broadcast.send(trade.to_spot_history_entry()).await {
                error!("failed to broadcast live Binance Spot trade {}: {}", payload.symbol, error);
            }
        }
    }
}

struct SwapKlineSaver {
    db: DuckTableTableChannel<KlinePo>,
    instrument_provider: InstrumentProvider,
    sync_broadcast: mpsc::Sender<HistoryEntry>,
}

impl SwapKlineSaver {
    fn new_for_sync(
        db: DuckTableTableChannel<KlinePo>,
        instrument_provider: InstrumentProvider,
        history_tx: mpsc::Sender<HistoryEntry>,
    ) -> ShareMessageHandler<BinanceSwapWebSocketStreamWrapper> {
        Arc::new(SwapKlineSaver {
            db,
            instrument_provider,
            sync_broadcast: history_tx,
        })
    }
}

#[async_trait]
impl MessageHandlerTrait<BinanceSwapWebSocketStreamWrapper> for SwapKlineSaver {
    async fn handle_message(&self, message: &BinanceSwapWebSocketStreamWrapper) {
        match &message.data {
            BinanceSwapWebSocketStreamResponse::Kline(_payload) => {
                if _payload.kline.is_close {
                    let Some(instrument) = self.instrument_provider.get_instrument(&_payload.kline.symbol) else {
                        error!("cannot save Binance Swap kline for unknown symbol {}", _payload.kline.symbol);
                        return;
                    };
                    let inst_id = instrument.id as u64;
                    let kline = KlinePo::from_swap_websocket_kline(_payload.kline.clone(), inst_id);
                    if let Err(e) = self.db.send(QueryCommand::Insert(InsertPayload::new_no_replay(kline.clone()))).await {
                        error!("Error save swap kline: {}", e);
                    }
                    if let Err(error) = self.sync_broadcast.send(kline.to_swap_history_entry()).await {
                        error!("failed to broadcast live Binance Swap kline {}: {}", _payload.kline.symbol, error);
                    }
                }
            }
            _ => {
                //ignore other message types
            }
        }
    }
}

///
/// 这个主要负责
/// 1. 更新symbol订阅的维护。比如上架和下架币
/// 2. 接受symbol信息，存入数据库。
///
pub struct KlineSubscribeService {}

impl KlineSubscribeService {
    ///
    /// 这里是
    /// 1. 开启websocket。监听kline。
    /// 2. 启动一个线程，监听symbol的更新。
    ///
    /// # symbol更新操作
    ///  1. 关闭之前的websocket连接
    ///  2. 重新订阅
    ///
    pub async fn startup_spot_sync(
        db: DuckTableTableChannel<KlinePo>,
        dashboard: Arc<BinanceDashboard>,
        proxy: Option<String>,
        interval: HistoryInterval,
        history_tx: mpsc::Sender<HistoryEntry>,
    ) -> Result<(), YuError> {
        let ws_url = SPOT_STREAM_WEBSOCKET;
        let instrument_provider = SpotInstrumentProvider::new_provider(dashboard);
        let saver = SpotKlineSaver::new_for_sync(db, instrument_provider.clone(), history_tx.clone());
        Self::start_listen_kline(instrument_provider, proxy, interval, DEFAULT_RECONNECT_INTERVAL, ws_url, saver, None).await
    }

    pub async fn startup_swap_sync(
        db: DuckTableTableChannel<KlinePo>,
        dashboard: Arc<BinanceDashboard>,
        proxy: Option<String>,
        interval: HistoryInterval,
        history_tx: mpsc::Sender<HistoryEntry>,
    ) -> Result<(), YuError> {
        let ws_url = SWAP_MARKET_STREAM_WEBSOCKET;
        let instrument_provider = SwapInstrumentProvider::new_provider(dashboard);
        let saver = SwapKlineSaver::new_for_sync(db, instrument_provider.clone(), history_tx);
        Self::start_listen_kline(instrument_provider, proxy, interval, DEFAULT_RECONNECT_INTERVAL, ws_url, saver, None).await
    }

    async fn start_listen_kline<M: WebSocketMessage>(
        instrument_provider: InstrumentProvider,
        proxy: Option<String>,
        interval: HistoryInterval,
        reconnect_interval: Duration,
        ws_url: &str,
        saver: ShareMessageHandler<M>,
        quote_asset: Option<String>,
    ) -> Result<(), YuError> {
        let proxy_clone = proxy.clone();

        let mut interface = Self::subscribe_trading_kline(
            instrument_provider.clone(),
            proxy,
            ws_url,
            saver.clone(),
            interval.clone(),
            quote_asset.clone(),
        )
        .await
        .map_err(|e| YuError::new(format!("启动监听:{}失败，因为:{}", ws_url, e).as_str()))?;

        let ws_for_reconnect = ws_url.to_string();
        let asset_for_reconnect = quote_asset.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(reconnect_interval).await;
                debug!("按周期重新订阅 Binance {:?} Kline", instrument_provider.instrument_type());

                match Self::subscribe_trading_kline(
                    instrument_provider.clone(),
                    proxy_clone.clone(),
                    ws_for_reconnect.as_str(),
                    saver.clone(),
                    interval.clone(),
                    asset_for_reconnect.clone(),
                )
                .await
                {
                    Ok(new_interface) => {
                        let command_sender = interface.command_sender();
                        match Self::close_connection(command_sender, 10).await {
                            Ok(_) => interface = new_interface,
                            Err(e) => {
                                new_interface
                                    .command_sender()
                                    .send(CommandMessage::Connection(ConnectionAction::Close))
                                    .ok();
                                error!("关闭之前连接发送消息失败，跳过此次重连可能会导致资源泄露: {}", e);
                            }
                        }
                    }
                    Err(e) => error!(
                        "重新订阅 Binance {:?} Kline 失败，保留当前连接: {}",
                        instrument_provider.instrument_type(),
                        e
                    ),
                }
            }
        });
        Ok(())
    }

    async fn close_connection(command_sender: UnboundedSender<CommandMessage>, max_retries: usize) -> Result<(), YuError> {
        let mut attempt = 0usize;
        loop {
            attempt += 1;
            // 每次构造新的命令实例，这样即使 CommandMessage 没有 Clone 也可以重试
            let cmd = CommandMessage::Connection(ConnectionAction::Close);

            match command_sender.send(cmd) {
                Ok(_) => {
                    debug!("Sent Close command to previous connection (attempt {})", attempt);
                    return Ok(());
                }
                Err(e) => {
                    // 记录更详细的信息：不仅打印错误，还打印将要发送的命令（可被 Debug 展示）
                    // 这样能避免日志仅显示 `SendError { .. }` 无法追踪要发送的 payload 的情况
                    error!(
                        "Error close prev connection on attempt {}: error={}. CommandMessage being sent: ",
                        attempt, e
                    );

                    if attempt >= max_retries {
                        error!("Giving up closing previous connection after {} attempts. Last error: {}", max_retries, e);
                        return Err(YuError::new("max retries").into());
                    } else {
                        // 简单指数退避：500ms * attempt
                        let jitter = Jitter::up_to(Duration::from_millis(500));
                        tokio::time::sleep(jitter + Duration::ZERO).await;
                    }
                }
            }
        }
    }

    async fn subscribe_trading_kline<M: WebSocketMessage>(
        instrument_provider: InstrumentProvider,
        proxy: Option<String>,
        ws_url: &str,
        handler: ShareMessageHandler<M>,
        interval: HistoryInterval,
        quote_asset: Option<String>,
    ) -> Result<Arc<WebSocketInterface<M>>, YuError> {
        let symbols = instrument_provider.instruments(quote_asset);
        let final_url = format!("{}?streams={}", ws_url, Self::compose_kline_url(&symbols, interval));
        debug!("Connecting to {}", final_url);
        let interface = WebSocketConnection::run::<M>(final_url, None, proxy, Some(handler)).await;

        info!(
            "initial subscribe {:?} symbols num: {:?}",
            instrument_provider.instrument_type(),
            symbols.len()
        );

        Ok(interface)
    }

    fn build_streams(symbols: &[BinanceInstrument], interval: HistoryInterval) -> Vec<String> {
        symbols
            .iter()
            .map(|s| format!("{}@kline_{}", s.symbol.to_lowercase(), interval.as_ref()))
            .collect()
    }

    ///
    /// 按照websocket订阅的格式，组成新的url的path
    /// 格式为
    /// <streamName1>/<streamName2>/<streamName3>
    /// 而kline的streamName的格式为 <symbol>@kline_<interval>
    /// symbol为TradingSymbol中的symbol
    /// interval为5m
    ///
    fn compose_kline_url(symbols: &[BinanceInstrument], interval: HistoryInterval) -> String {
        // 如果 symbols 为空，返回空字符串
        if symbols.is_empty() {
            return String::new();
        }

        // build_streams 会返回类似 `symbol@kline_5m` 的名称列表，按 Binance websocket 合并流的 path 要用 `/` 拼接
        Self::build_streams(symbols, interval).join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instrument(symbol: &str) -> BinanceInstrument {
        BinanceInstrument {
            id: 1,
            symbol: symbol.to_string(),
            status: "TRADING".to_string(),
            base_asset: symbol.trim_end_matches("USDT").to_string(),
            quote_asset: "USDT".to_string(),
            quote_asset_precision: 8,
            order_types: vec!["LIMIT".to_string()],
            symbol_type: yue::models::InstrumentType::Spot,
            on_board_time: None,
        }
    }

    /// 测试目的：验证 `compose_kline_url` 对空交易对列表的处理。
    /// 准备数据/过程：传入空的 BinanceInstrument slice，并使用 FiveMinutes 周期。
    /// 预期结果：返回空字符串。
    #[test]
    fn test_compose_kline_url_empty() {
        let symbols: Vec<BinanceInstrument> = vec![];
        let url = KlineSubscribeService::compose_kline_url(&symbols, HistoryInterval::FiveMinutes);
        assert_eq!(url, "");
    }

    /// 测试目的：验证单个交易对会转换为 Binance Kline stream 名称。
    /// 准备数据/过程：构造 BTCUSDT instrument，调用 `compose_kline_url` 使用 FiveMinutes 周期。
    /// 预期结果：返回 `btcusdt@kline_5m`。
    #[test]
    fn test_compose_kline_url_single() {
        let symbols = vec![instrument("BTCUSDT")];
        let url = KlineSubscribeService::compose_kline_url(&symbols, HistoryInterval::FiveMinutes);
        assert_eq!(url, "btcusdt@kline_5m");
    }

    /// 测试目的：验证多个交易对的 stream 名称按输入顺序拼接。
    /// 准备数据/过程：依次构造 BTCUSDT 和 ETHUSDT instrument，调用 `compose_kline_url`。
    /// 预期结果：返回两个以 `/` 分隔且顺序不变的 Kline stream 名称。
    #[test]
    fn test_compose_kline_url_multiple() {
        let symbols = vec![instrument("BTCUSDT"), instrument("ETHUSDT")];
        let url = KlineSubscribeService::compose_kline_url(&symbols, HistoryInterval::FiveMinutes);
        assert_eq!(url, "btcusdt@kline_5m/ethusdt@kline_5m");
    }
}
