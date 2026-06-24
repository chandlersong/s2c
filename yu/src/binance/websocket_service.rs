use crate::binance::bn_dashboard::{BinanceDashboardSnapShot, BinanceDashboardWatcher};
use crate::binance::bn_duck_db::DuckTableTableChannel;
use crate::binance::models::po::KlinePo;
use crate::errors::YuError;
use crate::errors::YuError::NotSupportError;
use async_trait::async_trait;
use governor::Jitter;
use li::websocket::connection::{
    CommandMessage, ConnectionAction, MessageHandlerTrait, ShareMessageHandler, WebSocketConnection, WebSocketInterface,
};
use li::websocket::models::WebSocketMessage;
use log::{debug, error, info};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use yue::binance::bn_json_websocket::{SPOT_STREAM_WEBSOCKET, SWAP_MARKET_STREAM_WEBSOCKET};
use yue::binance::bn_models::common::{SymbolInfo, SymbolType};
use yue::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamResponse::Kline;
use yue::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamWrapper;
use yue::binance::bn_models::swap_websocket_stream::{BinanceSwapWebSocketStreamResponse, BinanceSwapWebSocketStreamWrapper};
use yue::models::HistoryInterval;
use yue::query_message::{InsertPayload, QueryCommand};
///
/// 这个服务，主要后段，负责和websocket通行的一些service
///
///

struct SpotKlineSaver {
    db: DuckTableTableChannel<KlinePo>,
}

impl SpotKlineSaver {
    fn new(db: DuckTableTableChannel<KlinePo>) -> ShareMessageHandler<BinanceSpotWebSocketStreamWrapper> {
        Arc::new(SpotKlineSaver { db })
    }
}
#[async_trait]
impl MessageHandlerTrait<BinanceSpotWebSocketStreamWrapper> for SpotKlineSaver {
    async fn handle_message(&self, message: &BinanceSpotWebSocketStreamWrapper) {
        match &message.data {
            Kline(payload) => {
                if payload.kline.is_closed {
                    if let Err(e) = self
                        .db
                        .send(QueryCommand::Insert(InsertPayload::new_no_replay(KlinePo::from(payload.kline.clone()))))
                        .await
                    {
                        error!("Error save spot kline: {}", e);
                    }
                }
            }
            _ => {
                //ignore other message types
            }
        }
    }
}

struct SwapKlineSaver {
    db: DuckTableTableChannel<KlinePo>,
}

impl SwapKlineSaver {
    fn new(db: DuckTableTableChannel<KlinePo>) -> ShareMessageHandler<BinanceSwapWebSocketStreamWrapper> {
        Arc::new(SwapKlineSaver { db })
    }
}

#[async_trait]
impl MessageHandlerTrait<BinanceSwapWebSocketStreamWrapper> for SwapKlineSaver {
    async fn handle_message(&self, message: &BinanceSwapWebSocketStreamWrapper) {
        match &message.data {
            BinanceSwapWebSocketStreamResponse::Kline(_payload) => {
                match &message.data {
                    BinanceSwapWebSocketStreamResponse::Kline(_payload) => {
                        if _payload.kline.is_close {
                            if let Err(e) = self
                                .db
                                .send(QueryCommand::Insert(InsertPayload::new_no_replay(KlinePo::from(_payload.kline.clone()))))
                                .await
                            {
                                error!("Error save spot kline: {}", e);
                            }
                        }
                    }
                    _ => {
                        //ignore other message types
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
    pub async fn startup_spot(
        db: DuckTableTableChannel<KlinePo>,
        symbol_watch: BinanceDashboardWatcher,
        proxy: Option<String>,
        interval: HistoryInterval,
    ) -> Result<(), YuError> {
        let ws_url = SPOT_STREAM_WEBSOCKET;
        let saver = SpotKlineSaver::new(db);
        if let Some(value) = Self::start_listen_kline(SymbolType::Spot, proxy, interval, ws_url, &symbol_watch, saver).await {
            return value;
        }

        Ok(())
    }

    pub async fn startup_swap(
        db: DuckTableTableChannel<KlinePo>,
        symbol_watch: BinanceDashboardWatcher,
        proxy: Option<String>,
        interval: HistoryInterval,
    ) -> Result<(), YuError> {
        let ws_url = SWAP_MARKET_STREAM_WEBSOCKET;
        let saver = SwapKlineSaver::new(db);
        if let Some(value) = Self::start_listen_kline(SymbolType::Swap, proxy, interval, ws_url, &symbol_watch, saver).await {
            return value;
        }

        Ok(())
    }

    async fn start_listen_kline<M: WebSocketMessage>(
        symbol_type: SymbolType,
        proxy: Option<String>,
        interval: HistoryInterval,
        ws_url: &str,
        symbol_watch: &BinanceDashboardWatcher,
        saver: ShareMessageHandler<M>,
    ) -> Option<Result<(), YuError>> {
        let mut rx = symbol_watch.subscribe();
        let snapshot = (*rx.borrow()).clone();
        let proxy_clone = proxy.clone();

        let mut interface = match Self::subscribe_trading_kline(symbol_type, proxy, ws_url, &snapshot, saver.clone(), interval.clone()).await {
            Ok(interface) => interface,
            Err(e) => {
                return Some(Err(YuError::new(format!("启动监听:{}失败，因为:{}", ws_url, e).as_str()).into()));
            }
        };

        let ws_for_reconnect = ws_url.to_string();
        //每次symbol更新，重新订阅
        tokio::spawn(async move {
            // 更新的逻辑。主要保持一致有一条连接在。这种做法，
            // 可能会导致重叠，然后丢一点数据。因为db flush的时候，可能引起duplicate key。
            // 概率极低
            // 1.根据建立新连接
            // 2.如果成功，那么就关闭老连接
            // 3.建立失败，就用老连接。
            let update_proxy = proxy_clone;
            loop {
                match rx.changed().await {
                    Ok(_) => {
                        let snapshot = (*rx.borrow()).clone();
                        info!("开始重新订阅: 现在symbol数目是:{}", snapshot.spot_trading_symbols.len());

                        match Self::subscribe_trading_kline(
                            symbol_type,
                            update_proxy.clone(),
                            ws_for_reconnect.as_str(),
                            &snapshot,
                            saver.clone(),
                            interval.clone(),
                        )
                        .await
                        {
                            Ok(new_interface) => {
                                //建立成功，开始关闭老连接
                                let command_sender = interface.command_sender();
                                match Self::close_connection(command_sender, 10).await {
                                    Ok(_) => {
                                        //关闭成功，换了连接
                                        interface = new_interface;
                                    }
                                    Err(e) => {
                                        //关闭失败，保持老连接
                                        new_interface
                                            .command_sender()
                                            .send(CommandMessage::Connection(ConnectionAction::Close))
                                            .ok();
                                        error!("关闭之前连接发送消息失败，跳过此次的做法可能会导致资源泄露，建议调查原因并修复: {}", e);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(_) => {}
                }
            }
        });
        None
    }

    async fn close_connection(command_sender: UnboundedSender<CommandMessage>, max_retries: usize) -> Result<(), YuError> {
        let mut attempt = 0usize;
        loop {
            attempt += 1;
            // 每次构造新的命令实例，这样即使 CommandMessage 没有 Clone 也可以重试
            let cmd = CommandMessage::Connection(ConnectionAction::Close);

            match command_sender.send(cmd) {
                Ok(_) => {
                    info!("Sent Close command to previous connection (attempt {})", attempt);
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
        symbol_type: SymbolType,
        proxy: Option<String>,
        ws_url: &str,
        snapshot: &Arc<BinanceDashboardSnapShot>,
        handler: ShareMessageHandler<M>,
        interval: HistoryInterval,
    ) -> Result<Arc<WebSocketInterface<M>>, YuError> {
        let symbols = match symbol_type {
            SymbolType::Spot => &snapshot.spot_trading_symbols,
            SymbolType::Swap => &snapshot.swap_trading_symbols,
            _ => {
                return Err(NotSupportError(format!("symbol type {:?} not support", symbol_type)).into());
            }
        };
        let reconnect_interval = Duration::from_secs(5);
        let final_url = format!("{}?streams={}", ws_url, Self::compose_kline_url(symbols, interval));
        debug!("Connecting to {}", final_url);
        let interface = WebSocketConnection::run::<M>(final_url, reconnect_interval, proxy, Some(handler)).await;

        info!("initial subscribe symbols num: {:?}", symbols.len());

        Ok(interface)
    }

    fn build_streams(symbols: &[SymbolInfo], interval: HistoryInterval) -> Vec<String> {
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
    fn compose_kline_url(symbols: &[SymbolInfo], interval: HistoryInterval) -> String {
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
    use yue::binance::bn_models::common::SymbolInfo;

    /// 单元测试：测试 `compose_kline_url` 对空输入的返回值。
    /// 预期行为：传入空 slice 时返回空字符串。
    #[test]
    fn test_compose_kline_url_empty() {
        let symbols: Vec<SymbolInfo> = vec![];
        let url = KlineSubscribeService::compose_kline_url(&symbols, HistoryInterval::FiveMinutes);
        assert_eq!(url, "");
    }

    /// 单元测试：测试 `compose_kline_url` 对单个 symbol 的返回值。
    /// 预期行为：返回单个 stream 名称，例如 "btcusdt@kline_5m"。
    #[test]
    fn test_compose_kline_url_single() {
        let symbols = vec![SymbolInfo {
            symbol: "BTCUSDT".to_string(),
            on_board_time: None,
            quote_asset: "USDT".to_string(),
            quote_asset_precision: 0,
            order_types: vec![],
            status: "TRADING".to_string(),
            base_asset: "".to_string(),
            symbol_type: "".to_string(),
        }];
        let url = KlineSubscribeService::compose_kline_url(&symbols, HistoryInterval::FiveMinutes);
        assert_eq!(url, "btcusdt@kline_5m");
    }

    /// 单元测试：测试 `compose_kline_url` 对多个 symbol 的返回值。
    /// 预期行为：返回用 `/` 分隔的多个 stream 名称，且按传入顺序保持一致。
    #[test]
    fn test_compose_kline_url_multiple() {
        let symbols = vec![
            SymbolInfo {
                symbol: "BTCUSDT".to_string(),
                on_board_time: None,
                quote_asset: "USDT".to_string(),
                quote_asset_precision: 0,
                order_types: vec![],
                status: "TRADING".to_string(),
                base_asset: "".to_string(),
                symbol_type: "".to_string(),
            },
            SymbolInfo {
                symbol: "ETHUSDT".to_string(),
                on_board_time: None,
                quote_asset: "USDT".to_string(),
                quote_asset_precision: 0,
                order_types: vec![],
                status: "TRADING".to_string(),
                base_asset: "".to_string(),
                symbol_type: "".to_string(),
            },
        ];
        let url = KlineSubscribeService::compose_kline_url(&symbols, HistoryInterval::FiveMinutes);
        assert_eq!(url, "btcusdt@kline_5m/ethusdt@kline_5m");
    }
}
