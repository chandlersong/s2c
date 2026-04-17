use crate::binance::bn_dashboard::{BinanceDashboardSnapShot, BinanceDashboardWatcher};
use crate::binance::bn_duck_db::DuckTableTableChannel;
use crate::binance::models::po::KlinePo;
use crate::errors::YuError;
use crate::errors::YuError::NotSupportError;
use async_trait::async_trait;
use dashmap::DashMap;
use li::tools::time::{unix_2_readable, unix_time_now_u64_utc};
use li::websocket::connection::{
    CommandMessage, ConnectionAction, MessageHandler, MessageHandlerTrait, ShareMessageHandler, WebSocketConnection, WebSocketInterface,
};
use li::websocket::models::WebSocketMessage;
use log::{debug, error, info};
use mockall::predicate::le;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;
use yue::binance::bn_json_websocket::{SPOT_STREAM_WEBSOCKET, SWAP_STREAM_WEBSOCKET};
use yue::binance::bn_models::common::{SymbolInfo, SymbolType};
use yue::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamResponse::Kline;
use yue::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamWrapper;
use yue::models::HistoryInterval;
use yue::query_message::{InsertPayload, QueryCommand};
///
/// 这个服务，主要后段，负责和websocket通行的一些service
///
///

struct SpotKlineSaver {
    db: DuckTableTableChannel<KlinePo>,
    last_update: DashMap<String, AtomicU64>,
}

impl SpotKlineSaver {
    fn new(db: DuckTableTableChannel<KlinePo>) -> ShareMessageHandler<BinanceSpotWebSocketStreamWrapper> {
        Arc::new(SpotKlineSaver {
            db,
            last_update: DashMap::new(),
        })
    }
    /// 比较并在必要时更新 map 中的时间戳。
    ///
    /// 返回值：
    /// - true: 表示当前 close_time 与 map 中的值不同（或不存在），需要保存该 message
    /// - false: 表示 map 中已有相同的 close_time，应该跳过保存
    fn need_save(&self, symbol: &str, close_time: u64) -> bool {
        use dashmap::mapref::entry::Entry;
        use std::sync::atomic::Ordering;

        match self.last_update.entry(symbol.to_string()) {
            Entry::Occupied(occ) => {
                let atomic = occ.get();
                // 使用 compare_exchange 循环，做到原子性的比较并更新
                loop {
                    let prev = atomic.load(Ordering::SeqCst);
                    if prev == close_time {
                        // 相同，跳过
                        return false;
                    }
                    match atomic.compare_exchange(prev, close_time, Ordering::SeqCst, Ordering::SeqCst) {
                        Ok(_) => return true, // 更新成功，需保存
                        Err(_) => continue,   // 竞争发生，重试
                    }
                }
            }
            Entry::Vacant(vac) => {
                vac.insert(AtomicU64::new(close_time));
                true
            }
        }
    }
}
#[async_trait]
impl MessageHandlerTrait<BinanceSpotWebSocketStreamWrapper> for SpotKlineSaver {
    async fn handle_message(&self, message: &BinanceSpotWebSocketStreamWrapper) {
        match &message.data {
            Kline(payload) => {
                if payload.kline.is_closed {
                    let symbol = payload.kline.symbol.clone();
                    let start_time = payload.kline.start_time;

                    // FUTURE: 发现close的Kline都会重复发的。需要写一个版本，两个都存，然后比较一下有没有区别。
                    //TODO： 找寻问题，如果解决，改成debug。甚至删除代码。
                    if !self.need_save(&symbol, start_time) {
                        info!("{} 在 {} 重复发送!", symbol, unix_2_readable(&start_time));
                        return;
                    }

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
        interval: HistoryInterval,
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
        let interface = match Self::subscribe_trading_kline(symbol_type, proxy, ws_url, &snapshot, saver.clone(), interval.clone()).await {
            Ok(interface) => interface,
            Err(e) => {
                return Err(YuError::new(format!("启动监听:{}失败，因为:{}", ws_url, e).as_str()).into());
            }
        };

        //每次symbol更新，重新订阅
        tokio::spawn(async move {
            let update_proxy = proxy_clone;
            loop {
                match rx.changed().await {
                    Ok(_) => {
                        let snapshot = (*rx.borrow()).clone();
                        info!("开始重新订阅: 现在symbol数目是:{}", snapshot.spot_trading_symbols.len());
                        let command_sender = interface.command_sender();
                        if let Err(e) = command_sender.send(CommandMessage::Connection(ConnectionAction::Close)) {
                            error!("Error close prev connection: {:?}", e);
                        };
                        if let Err(e) =
                            Self::subscribe_trading_kline(symbol_type, update_proxy.clone(), ws_url, &snapshot, saver.clone(), interval.clone()).await
                        {
                            error!("Error subscribing to kline: {:?}", e);
                        };
                    }
                    Err(_) => {}
                }
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
    use tokio::sync::mpsc;
    use yue::binance::bn_models::common::SymbolInfo;

    /// 单元测试：测试 `SpotKlineSaver::need_save` 的行为
    #[test]
    fn test_need_save_logic() {
        // 构造一个假的 db sender，用于创建 SpotKlineSaver 实例
        let (tx, _rx) = mpsc::channel::<QueryCommand<KlinePo>>(4);
        let saver = SpotKlineSaver {
            db: tx,
            last_update: DashMap::new(),
        };

        // 初次插入应返回 true（需要保存）
        assert!(saver.need_save("BTCUSDT", 1000));

        // 相同时间再次判断应返回 false（跳过）
        assert!(!saver.need_save("BTCUSDT", 1000));

        // 不同时间应返回 true（更新并保存）
        assert!(saver.need_save("BTCUSDT", 2000));

        // 更新后再次用相同时间返回 false
        assert!(!saver.need_save("BTCUSDT", 2000));

        // 不同的 symbol 不相互影响
        assert!(saver.need_save("ETHUSDT", 3000));
        assert!(!saver.need_save("ETHUSDT", 3000));
    }

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
