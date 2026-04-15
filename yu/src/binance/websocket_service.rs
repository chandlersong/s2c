use crate::binance::bn_dashboard::{BinanceDashboardSnapShot, BinanceDashboardWatcher, TradingSymbol};
use crate::errors::YuError;
use crate::errors::YuError::NotSupportError;
use async_trait::async_trait;
use li::websocket::connection::{MessageHandler, WebSocketConnection, WebSocketInterface};
use li::websocket::models::WebSocketMessage;
use log::{error, info};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use yue::binance::bn_json_websocket::{SPOT_STREAM_WEBSOCKET, SWAP_STREAM_WEBSOCKET, WS_SUBSCRIBE_COMMAND};
use yue::binance::bn_models::common::SymbolType;
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamResponse::Kline;
use yue::binance::bn_models::spot_websocket_stream::{BinanceSpotWebSocketStreamWrapper, SpotKlineData};
use yue::query_message::{DataSourceExecutor, InsertPayload, QueryCommand};

///
/// 这个服务，主要后段，负责和websocket通行的一些service
///
///

struct SpotKlineSaver {
    db: DataSourceExecutor<SpotKlineData>,
}
#[async_trait]
impl MessageHandler<BinanceSpotWebSocketStreamWrapper> for SpotKlineSaver {
    async fn handle_message(&self, message: &BinanceSpotWebSocketStreamWrapper) {
        match &message.data {
            Kline(payload) => {
                if payload.kline.is_closed {
                    let insert_payload = InsertPayload::new_no_replay(payload.kline.clone());
                    if let Err(e) = self.db.send(QueryCommand::Insert(insert_payload)).await {
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
    /// 1. 根据symbol_type定位一个确定URL
    /// 2. 订阅相应的信息。
    /// 3. 保存进数据库
    ///
    pub async fn startup(
        symbol_type: SymbolType,
        db: DataSourceExecutor<SpotKlineData>,
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
        tokio::spawn(async move {
            match rx.changed().await {
                Ok(_) => {}
                Err(_) => {}
            }
        });
        if let Err(e) = Self::subscribe_trading_kline(symbol_type, proxy, ws_url, &snapshot).await {
            error!("Error subscribing to trading {} kline: {:?}", symbol_type, e);
            return Err(YuError::from(e));
        }

        Ok(())
    }

    async fn subscribe_trading_kline(
        symbol_type: SymbolType,
        proxy: Option<String>,
        ws_url: &str,
        snapshot: &Arc<BinanceDashboardSnapShot>,
    ) -> Result<(), YuError> {
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
        let interface = WebSocketConnection::run::<BinanceSpotWebSocketStreamWrapper>(final_url, reconnect_interval, proxy, None).await;

        info!("initial subscribe symbols num: {:?}", symbols.len());

        Ok(())
    }

    fn build_streams(symbols: &[TradingSymbol]) -> Vec<String> {
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
    fn compose_kline_url(symbols: &[TradingSymbol]) -> String {
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

    /// 单元测试：测试 `compose_kline_url` 对空输入的返回值。
    /// 预期行为：传入空 slice 时返回空字符串。
    #[test]
    fn test_compose_kline_url_empty() {
        let symbols: Vec<TradingSymbol> = vec![];
        let url = KlineSubscribeService::compose_kline_url(&symbols);
        assert_eq!(url, "");
    }

    /// 单元测试：测试 `compose_kline_url` 对单个 symbol 的返回值。
    /// 预期行为：返回单个 stream 名称，例如 "btcusdt@kline_5m"。
    #[test]
    fn test_compose_kline_url_single() {
        let symbols = vec![TradingSymbol {
            symbol: "BTCUSDT".to_string(),
            on_board_time: None,
            quote_asset: "USDT".to_string(),
            status: "TRADING".to_string(),
        }];
        let url = KlineSubscribeService::compose_kline_url(&symbols);
        assert_eq!(url, "btcusdt@kline_5m");
    }

    /// 单元测试：测试 `compose_kline_url` 对多个 symbol 的返回值。
    /// 预期行为：返回用 `/` 分隔的多个 stream 名称，且按传入顺序保持一致。
    #[test]
    fn test_compose_kline_url_multiple() {
        let symbols = vec![
            TradingSymbol {
                symbol: "BTCUSDT".to_string(),
                on_board_time: None,
                quote_asset: "USDT".to_string(),
                status: "TRADING".to_string(),
            },
            TradingSymbol {
                symbol: "ETHUSDT".to_string(),
                on_board_time: None,
                quote_asset: "USDT".to_string(),
                status: "TRADING".to_string(),
            },
        ];
        let url = KlineSubscribeService::compose_kline_url(&symbols);
        assert_eq!(url, "btcusdt@kline_5m/ethusdt@kline_5m");
    }
}
