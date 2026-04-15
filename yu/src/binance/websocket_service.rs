use crate::binance::bn_dashboard::{BinanceDashboardSnapShot, BinanceDashboardWatcher};
use crate::binance::bn_duck_db::{BinanceKlineDataExecutor, DuckTableTableChannel};
use crate::binance::models::po::KlinePo;
use crate::errors::YuError;
use crate::errors::YuError::NotSupportError;
use async_trait::async_trait;
use li::tools::time::{current_date_string, unix_2_readable, unix_time_now_u64_utc};
use li::websocket::connection::{
    CommandMessage, ConnectionAction, MessageHandler, MessageHandlerTrait, ShareMessageHandler, WebSocketConnection, WebSocketInterface,
};
use li::websocket::models::WebSocketMessage;
use log::{error, info};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use yue::binance::bn_json_websocket::{SPOT_STREAM_WEBSOCKET, SWAP_STREAM_WEBSOCKET, WS_SUBSCRIBE_COMMAND};
use yue::binance::bn_models::common::{SymbolInfo, SymbolType};
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamResponse::Kline;
use yue::binance::bn_models::spot_websocket_stream::{BinanceSpotWebSocketStreamWrapper, SpotKlineData};
use yue::query_message::{DataSourceExecutorTrait, InsertPayload, QueryCommand};

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
                    info!(
                        "received closed {} kline at {}",
                        payload.symbol,
                        unix_2_readable(&unix_time_now_u64_utc())
                    );
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
        symbol_type: SymbolType,
        db: DuckTableTableChannel<KlinePo>,
        symbol_watch: BinanceDashboardWatcher,
        proxy: Option<String>,
    ) -> Result<(), YuError> {
        let ws_url = match symbol_type {
            SymbolType::Spot => SPOT_STREAM_WEBSOCKET,
            SymbolType::Swap => SWAP_STREAM_WEBSOCKET,
            _ => {
                return Err(NotSupportError(format!("symbol type {:?} not support", symbol_type)).into());
            }
        };
        let mut rx = symbol_watch.subscribe();
        let snapshot = (*rx.borrow()).clone();
        let proxy_clone = proxy.clone();
        let saver = SpotKlineSaver::new(db);
        let interface = match Self::subscribe_trading_kline(symbol_type, proxy, ws_url, &snapshot, saver.clone()).await {
            Ok(interface) => interface,
            Err(e) => {
                return Err(YuError::new(format!("启动监听:{}失败，因为:{}", ws_url, e).as_str()).into());
            }
        };

        //每次symbol更新，重新订阅
        tokio::spawn(async move {
            match rx.changed().await {
                Ok(_) => {
                    let snapshot = (*rx.borrow_and_update()).clone();
                    let command_sender = interface.command_sender();
                    if let Err(e) = command_sender.send(CommandMessage::Connection(ConnectionAction::Close)) {
                        error!("Error close prev connection: {:?}", e);
                    };
                    if let Err(e) = Self::subscribe_trading_kline(symbol_type, proxy_clone, ws_url, &snapshot, saver.clone()).await {
                        error!("Error subscribing to kline: {:?}", e);
                    };
                }
                Err(_) => {}
            }
        });

        Ok(())
    }

    async fn subscribe_trading_kline<M: WebSocketMessage>(
        symbol_type: SymbolType,
        proxy: Option<String>,
        ws_url: &str,
        snapshot: &Arc<BinanceDashboardSnapShot>,
        handler: ShareMessageHandler<M>,
    ) -> Result<Arc<WebSocketInterface<M>>, YuError> {
        let symbols = match symbol_type {
            SymbolType::Spot => &snapshot.spot_trading_symbols,
            SymbolType::Swap => &snapshot.swap_trading_symbols,
            _ => {
                return Err(NotSupportError(format!("symbol type {:?} not support", symbol_type)).into());
            }
        };
        let reconnect_interval = Duration::from_secs(5);
        let final_url = format!("{}?streams={}", ws_url, Self::compose_kline_url(symbols));
        info!("Connecting to {}", final_url);
        let interface = WebSocketConnection::run::<M>(final_url, reconnect_interval, proxy, Some(handler)).await;

        info!("initial subscribe symbols num: {:?}", symbols.len());

        Ok(interface)
    }

    fn build_streams(symbols: &[SymbolInfo]) -> Vec<String> {
        symbols.iter().map(|s| format!("{}@kline_{}", s.symbol.to_lowercase(), "5m")).collect()
    }

    ///
    /// 按照websocket订阅的格式，组成新的url的path
    /// 格式为
    /// <streamName1>/<streamName2>/<streamName3>
    /// 而kline的streamName的格式为 <symbol>@kline_<interval>
    /// symbol为TradingSymbol中的symbol
    /// interval为5m
    ///
    fn compose_kline_url(symbols: &[SymbolInfo]) -> String {
        // 如果 symbols 为空，返回空字符串
        if symbols.is_empty() {
            return String::new();
        }

        // build_streams 会返回类似 `symbol@kline_5m` 的名称列表，按 Binance websocket 合并流的 path 要用 `/` 拼接
        Self::build_streams(symbols).join("/")
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
        let url = KlineSubscribeService::compose_kline_url(&symbols);
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
        let url = KlineSubscribeService::compose_kline_url(&symbols);
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
        let url = KlineSubscribeService::compose_kline_url(&symbols);
        assert_eq!(url, "btcusdt@kline_5m/ethusdt@kline_5m");
    }
}
