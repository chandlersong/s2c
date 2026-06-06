use crate::binance::bn_duck_db::DuckTableTableChannel;
use crate::binance::models::po::FundingRatePo;
use li::errors::LiError;
use li::tools::time::{unix_time_now_u64_utc, UnixTimeStamp};
use log::{error, info};
use serde::de::DeserializeOwned;
use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use yue::binance::bn_models::common::SymbolInfo;
use yue::binance::bn_models::spot_restful::ExchangeInfo;
use yue::binance::bn_models::swap_restful::SwapExchangeInfo;
use yue::binance::bn_restful_commands::{
    execute_json_request, BINANCE_SPOT_BASE, BINANCE_SWAP_BASE, SPOT_EXCHANGE_COMMAND, SPOT_RATE_PER_MINUTE, SWAP_EXCHANGE_COMMAND,
};
use yue::binance::order_book::{OrderBook, OrderBookService, OrderBookSnapshotMsg};
use yue::binance::restful_func::{get_trading_spot_symbols, get_trading_swap_symbols, CONTRACT_TYPE_PERPETUAL};
use yue::errors::YueError;
use yue::http_client::get_http_client;
use yue::models::HistoryInterval;

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
}

impl BinanceDashboard {
    pub fn new(data_retention_hours: u64) -> Self {
        BinanceDashboard {
            spot_symbols: Arc::new(RwLock::new(vec![])),
            swap_symbols: Arc::new(RwLock::new(vec![])),
            data_retention_hours,
            debug_mood: false,
        }
    }

    ///
    /// 主要本地的初始化的request的访问很长。所以写了一个debug模式。
    /// 所有的改动，手工调用
    /// Mock目录：
    /// 1. 各个的exchange info的update。从本地直接读取文件。
    ///
    #[deprecated]
    pub fn debug_mode(data_retention_hours: u64) -> Self {
        BinanceDashboard {
            spot_symbols: Arc::new(RwLock::new(vec![])),
            swap_symbols: Arc::new(RwLock::new(vec![])),
            data_retention_hours,
            debug_mood: true,
        }
    }

    pub fn new_with_data(spot_symbol: Vec<SymbolInfo>, swap_symbol: Vec<SymbolInfo>, data_retention_hours: u64) -> Self {
        BinanceDashboard {
            spot_symbols: Arc::new(RwLock::new(spot_symbol)),
            swap_symbols: Arc::new(RwLock::new(swap_symbol)),
            data_retention_hours,
            debug_mood: false,
        }
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

                let res = match self.refresh_all_symbol(spot_all, swap_res) {
                    Ok((spot_symbols, swap_symbols)) => {
                        let trading_spot_symbols: Vec<SymbolInfo> = spot_symbols.iter().filter(|s| s.status == "TRADING").cloned().collect();
                        let trading_swap_symbols: Vec<SymbolInfo> = swap_symbols.iter().filter(|s| s.status == "TRADING").cloned().collect();
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
