use crate::binance::bn_consts::BN_SYMBOL_STATUS_TRADING;
use crate::binance::duckdb_repository::{BNInstrumentRepository, get_instrument_repo};
use crate::binance::jobs::initial_tables;
use crate::binance::models::po::{BinanceInstrument, SWAP_TEXT, TRADIFI_PERPETUAL_TEXT};
use crate::duck_db::DuckDBDSProvider;
use crate::errors::YuError;
use li::errors::LiError;
use li::tools::time::{UnixTimeStamp, unix_time_now_u64_utc};
use log::{error, info};
use serde::de::DeserializeOwned;
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use yue::binance::bn_models::common::SymbolInfo;
use yue::binance::bn_models::spot_restful::ExchangeInfo;
use yue::binance::bn_models::swap_restful::SwapExchangeInfo;
use yue::binance::bn_restful_commands::{
    BINANCE_SPOT_BASE, BINANCE_SWAP_BASE, SPOT_EXCHANGE_COMMAND, SPOT_RATE_PER_MINUTE, SWAP_EXCHANGE_COMMAND, execute_json_request,
};
use yue::binance::restful_func::{get_trading_spot_symbols, get_trading_swap_symbols};
use yue::errors::YueError;
use yue::http_client::get_http_client;
use yue::models::HistoryInterval;
use yue::models::InstrumentType;

#[derive(Clone)]
pub struct BinanceDashboardSnapShot {
    pub spot_trading_symbols: Vec<SymbolInfo>,
    pub swap_trading_symbols: Vec<SymbolInfo>,
    pub timestamp: UnixTimeStamp,
}

impl BinanceDashboardSnapShot {
    pub fn new(spot_trading_symbols: Vec<SymbolInfo>, swap_trading_symbols: Vec<SymbolInfo>) -> Self {
        Self {
            spot_trading_symbols,
            swap_trading_symbols,
            timestamp: unix_time_now_u64_utc(),
        }
    }
}

pub type BinanceDashboardWatcher = watch::Sender<Arc<BinanceDashboardSnapShot>>;

//FUTURE：把这些存入数据库
///
/// 1. 放在结构体里面保存的为全部。
/// 2. 通过BinanceDashboardWatcher发送出去的为正在交易状态的symbol。
///
#[derive(Clone)]
pub struct BinanceDashboard {
    spot_symbols: Arc<RwLock<Vec<SymbolInfo>>>,
    swap_symbols: Arc<RwLock<Vec<SymbolInfo>>>,
    data_retention_hours: u64,
    debug_mood: bool,
    instrument_repo: BNInstrumentRepository,
}

impl BinanceDashboard {
    pub fn new(data_retention_hours: u64, provider: Option<DuckDBDSProvider>) -> Self {
        BinanceDashboard {
            spot_symbols: Arc::new(RwLock::new(vec![])),
            swap_symbols: Arc::new(RwLock::new(vec![])),
            data_retention_hours,
            debug_mood: false,
            instrument_repo: get_instrument_repo(provider),
        }
    }

    ///
    /// 主要本地的初始化的request的访问很长。所以写了一个debug模式。
    /// 所有的改动，手工调用
    /// Mock目录：
    /// 1. 各个的exchange info的update。从本地直接读取文件。
    ///
    #[deprecated]
    pub fn debug_mode(data_retention_hours: u64, instrument_repo: Option<BNInstrumentRepository>) -> Result<Self, YuError> {
        let inst_repo = match instrument_repo {
            None => {
                let provider = Some(DuckDBDSProvider::memory_db());
                initial_tables(provider.clone())?;
                get_instrument_repo(provider)
            }
            Some(repo) => repo,
        };
        Ok(BinanceDashboard {
            spot_symbols: Arc::new(RwLock::new(vec![])),
            swap_symbols: Arc::new(RwLock::new(vec![])),
            data_retention_hours,
            debug_mood: true,
            instrument_repo: inst_repo,
        })
    }

    pub fn new_with_data(spot_symbol: Vec<SymbolInfo>, swap_symbol: Vec<SymbolInfo>, data_retention_hours: u64) -> Result<Self, YuError> {
        let provider = Some(DuckDBDSProvider::memory_db());
        initial_tables(provider.clone())?;
        Ok(BinanceDashboard {
            spot_symbols: Arc::new(RwLock::new(spot_symbol)),
            swap_symbols: Arc::new(RwLock::new(swap_symbol)),
            data_retention_hours,
            debug_mood: false,
            instrument_repo: get_instrument_repo(provider),
        })
    }

    async fn query_spot_exchange_info() -> Result<ExchangeInfo, YueError> {
        let client = get_http_client();
        let rb = client.get(SPOT_EXCHANGE_COMMAND.as_ref().as_str());
        execute_json_request::<ExchangeInfo>(&SPOT_EXCHANGE_COMMAND, rb, None).await
    }

    async fn query_swap_exchange_info() -> Result<SwapExchangeInfo, YueError> {
        let client = get_http_client();
        let rb = client.get(SWAP_EXCHANGE_COMMAND.as_ref().as_str());
        execute_json_request::<SwapExchangeInfo>(&SWAP_EXCHANGE_COMMAND, rb, None).await
    }

    async fn refresh_rate_limit(spot_exchange: &ExchangeInfo, swap_exchange: &SwapExchangeInfo) {
        let spot_request_limit = spot_exchange
            .rate_limits
            .iter()
            .filter(|r| r.rate_limit_type == "REQUEST_WEIGHT" && r.interval == "MINUTE")
            .map(|r| r.limit)
            .next()
            .unwrap_or(SPOT_RATE_PER_MINUTE as i32);
        let swap_request_limit = swap_exchange
            .rate_limits
            .iter()
            .filter(|r| r.rate_limit_type == "REQUEST_WEIGHT" && r.interval == "MINUTE")
            .map(|r| r.limit)
            .next()
            .unwrap_or(SPOT_RATE_PER_MINUTE as i32);
        info!("refresh spot rate limit {},swap rate limit {}", spot_request_limit, swap_request_limit);
        BINANCE_SPOT_BASE.clone().refresh_rate_limit(spot_request_limit as u32, None).await;
        BINANCE_SWAP_BASE.clone().refresh_rate_limit(swap_request_limit as u32, None).await;
    }

    fn refresh_all_symbol(
        &self,
        spot_res: Result<Vec<SymbolInfo>, YueError>,
        swap_res: Result<Vec<SymbolInfo>, YueError>,
    ) -> Result<(Vec<SymbolInfo>, Vec<SymbolInfo>), LiError> {
        match (spot_res, swap_res) {
            (Ok(spot_symbols), Ok(swap_symbols)) => {
                *self.spot_symbols.write().unwrap() = spot_symbols.clone();
                *self.swap_symbols.write().unwrap() = swap_symbols.clone();
                Ok((spot_symbols, swap_symbols))
            }
            (Err(e), _) => {
                error!("Error fetching trading spot symbols: {:?}", e);
                Err(LiError::CustomError(format!("获取现货交易对失败: {}", e)))
            }
            (_, Err(e)) => {
                error!("Error fetching trading swap symbols: {:?}", e);
                Err(LiError::CustomError(format!("获取合约交易对失败: {}", e)))
            }
        }
    }

    pub fn read_spot_exchange_info_from_file_sync<P: AsRef<Path>, E: DeserializeOwned>(path: P) -> Result<E, YueError> {
        let s = fs::read_to_string(path)?; // std::io::Error -> YueError::IoError
        let info: E = serde_json::from_str(&s)?; // serde_json::Error -> YueError::SerdeError
        Ok(info)
    }
    pub async fn execute(&self) -> Result<Arc<BinanceDashboardSnapShot>, LiError> {
        let (spot_exchange, swap_exchange) = if self.debug_mood {
            let spot = Self::read_spot_exchange_info_from_file_sync("testdata/exchange_data/spot_exchange.json");
            let swap = Self::read_spot_exchange_info_from_file_sync("testdata/exchange_data/swap_exchange.json");
            (spot, swap)
        } else {
            let (spot_exchange, swap_exchange) = tokio::join!(Self::query_spot_exchange_info(), Self::query_swap_exchange_info());
            (spot_exchange, swap_exchange)
        };

        match (spot_exchange, swap_exchange) {
            (Ok(spot), Ok(swap)) => {
                // refresh limit
                //TODO：能正常下载后，再把这个功能加上。
                Self::refresh_rate_limit(&spot, &swap).await;
                // refresh symbol
                let spot_all = get_trading_spot_symbols(spot).await;
                let swap_res = get_trading_swap_symbols(swap, None).await;

                let swap_text = SWAP_TEXT.to_string();
                let tradifi_perpetual = TRADIFI_PERPETUAL_TEXT.to_string();
                let res = match self.refresh_all_symbol(spot_all, swap_res) {
                    Ok((spot_symbols, swap_symbols)) => {
                        let trading_spot_symbols: Vec<SymbolInfo> =
                            spot_symbols.iter().filter(|s| s.status == BN_SYMBOL_STATUS_TRADING).cloned().collect();
                        let trading_swap_symbols: Vec<SymbolInfo> = swap_symbols
                            .iter()
                            .filter(|s| s.status == BN_SYMBOL_STATUS_TRADING && (s.symbol_type == swap_text || s.symbol_type == tradifi_perpetual))
                            .cloned()
                            .collect();

                        if let Err(e) = self.refresh_instrument_in_db(&trading_spot_symbols, InstrumentType::Spot).await {
                            error!("Error persist trading spot symbols: {:?}", e);
                        }
                        if let Err(e) = self.refresh_instrument_in_db(&trading_swap_symbols, InstrumentType::Swap).await {
                            error!("Error persist trading swap symbols: {:?}", e);
                        }

                        BinanceDashboardSnapShot::new(trading_spot_symbols, trading_swap_symbols)
                    }
                    Err(e) => return Err(LiError::CustomError(format!("刷新交易符号失败: {}", e))),
                };

                Ok(Arc::new(res))
            }
            (Err(e), _) => Err(LiError::CustomError(format!("获取现货交易所信息失败: {}", e))),
            (_, Err(e)) => Err(LiError::CustomError(format!("获取永续交易所信息失败: {}", e))),
        }
    }

    ///
    /// # 步骤
    /// 1. 从数据库中，获取symbol和id的hashmap，后续称之symbols。
    /// 2. loop stops。判断symbol是否在存在于symbols.
    ///  - 如果不存在，则转换成po，存入数据库
    ///  - 如果存在，则跳过，在map中删除该条symbol
    /// 3. 把symbol中剩余的数据，标注为非交易
    pub async fn refresh_instrument_in_db(&self, symbols: &Vec<SymbolInfo>, inst_type: InstrumentType) -> Result<(), YuError> {
        let mut existing_symbols = self.instrument_repo.get_map_of_symbol_id(inst_type).await?;

        for symbol in symbols {
            if existing_symbols.remove(&symbol.symbol).is_none() {
                if let Err(e) = self.instrument_repo.insert_instrument(BinanceInstrument::from(symbol)).await {
                    error!(
                        "error adding instrument to symbol {} type is {}: {:?}",
                        symbol.symbol, symbol.symbol_type, e
                    );
                }
            }
        }

        self.instrument_repo
            .mark_instruments_not_trading(existing_symbols.into_values().collect())
            .await
    }

    pub fn spot_all_symbols(&self) -> Arc<RwLock<Vec<SymbolInfo>>> {
        self.spot_symbols.clone()
    }

    pub fn swap_all_symbols(&self) -> Arc<RwLock<Vec<SymbolInfo>>> {
        self.swap_symbols.clone()
    }

    ///
    /// 1. 获取当前时间。然后减去data_retention_hours，得到应该保留的最早时间戳
    /// 2. 根据interval调整时间戳进行调整
    ///
    /// interval: 默认值是五分钟
    /// 返回标准应该保留的最大时间
    ///
    pub fn get_earliest_timestamp(&self, interval: Option<HistoryInterval>) -> Option<u64> {
        // 获取当前 Unix 毫秒时间
        let now_ms = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_millis() as u64,
            Err(_) => return None,
        };

        // 计算保留时长对应的毫秒数（防溢出）
        let retention_ms = self.data_retention_hours.saturating_mul(3600).saturating_mul(1000);

        // 计算最早保留的时间戳（不小于0）
        let earliest = now_ms.saturating_sub(retention_ms);

        // 使用传入的 interval 对齐时间戳，默认使用 FiveMinutes
        let interval_to_use = interval.unwrap_or(HistoryInterval::FiveMinutes);
        let aligned = interval_to_use.get_close_unix_ms(earliest);

        Some(aligned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binance::bn_consts::{BN_SYMBOL_STATUS_BREAK, BN_SYMBOL_STATUS_NOT_TRADING};

    fn symbol(symbol: &str, status: &str) -> SymbolInfo {
        SymbolInfo {
            symbol: symbol.to_string(),
            status: status.to_string(),
            base_asset: symbol.trim_end_matches("USDT").to_string(),
            quote_asset: "USDT".to_string(),
            quote_asset_precision: 8,
            order_types: vec!["LIMIT".to_string()],
            symbol_type: "spot".to_string(),
            on_board_time: None,
        }
    }

    #[tokio::test]
    async fn refresh_instrument_in_db_inserts_new_and_marks_missing_symbols_not_trading() {
        let dashboard = BinanceDashboard::new_with_data(Vec::new(), Vec::new(), 24).unwrap();
        let old_active = BinanceInstrument::from(&symbol("BTCUSDT", BN_SYMBOL_STATUS_BREAK));
        let missing = BinanceInstrument::from(&symbol("ETHUSDT", BN_SYMBOL_STATUS_TRADING));
        dashboard.instrument_repo.insert_instrument(old_active.clone()).await.unwrap();
        dashboard.instrument_repo.insert_instrument(missing.clone()).await.unwrap();

        dashboard
            .refresh_instrument_in_db(
                &vec![
                    symbol("BTCUSDT", BN_SYMBOL_STATUS_TRADING),
                    symbol("SOLUSDT", BN_SYMBOL_STATUS_TRADING),
                    symbol("XRPUSDT", BN_SYMBOL_STATUS_TRADING),
                ],
                InstrumentType::Spot,
            )
            .await
            .unwrap();

        let instruments = dashboard.instrument_repo.get_instrument_by_type(InstrumentType::Spot).await.unwrap();
        assert_eq!(instruments.len(), 4);
        let btc = instruments.iter().find(|instrument| instrument.symbol == "BTCUSDT").unwrap();
        assert_eq!(btc.id, old_active.id);
        assert_eq!(btc.status, BN_SYMBOL_STATUS_BREAK);
        assert!(
            instruments
                .iter()
                .any(|instrument| instrument.symbol == "SOLUSDT" && instrument.status == BN_SYMBOL_STATUS_TRADING)
        );
        assert!(
            instruments
                .iter()
                .any(|instrument| instrument.symbol == "XRPUSDT" && instrument.status == BN_SYMBOL_STATUS_TRADING)
        );
        let eth = instruments.iter().find(|instrument| instrument.symbol == "ETHUSDT").unwrap();
        assert_eq!(eth.id, missing.id);
        assert_eq!(eth.status, BN_SYMBOL_STATUS_NOT_TRADING);
    }
}
