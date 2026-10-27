use crate::binance::bn_consts::BN_SYMBOL_STATUS_TRADING;
use crate::binance::db_consts::initial_tables;
use crate::binance::duckdb_repository::{BNInstrumentRepository, get_instrument_repo_for_production};
use crate::binance::models::po::BinanceInstrument;
use crate::duck_db::DuckDBDSProvider;
use crate::errors::YuError;
use li::tools::time::{UnixTimeStamp, unix_time_now_u64_utc};
use log::{error, info};
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use yue::binance::bn_models::common::SymbolInfoTrait;
use yue::binance::bn_models::spot_restful::ExchangeInfo;
use yue::binance::bn_models::swap_restful::SwapExchangeInfo;
use yue::binance::bn_restful_commands::{
    BINANCE_SPOT_BASE, BINANCE_SWAP_BASE, SPOT_EXCHANGE_COMMAND, SPOT_RATE_PER_MINUTE, SWAP_EXCHANGE_COMMAND, execute_json_request,
};
use yue::errors::YueError;
use yue::http_client::get_http_client;
use yue::models::InstrumentType;
use yue::models::{HistoryInterval, USDT_TEXT};

pub trait KlineInstrumentProviderTrait: Send + Sync {
    fn instrument_type(&self) -> InstrumentType;
    fn instruments(&self, quote_asset: Option<String>) -> Vec<BinanceInstrument>;
    fn get_instrument(&self, symbol: &str) -> Option<BinanceInstrument>;
}

pub type InstrumentProvider = Arc<dyn KlineInstrumentProviderTrait>;

pub struct SpotInstrumentProvider {
    dashboard: Arc<BinanceDashboard>,
}

impl SpotInstrumentProvider {
    pub fn new_provider(dashboard: Arc<BinanceDashboard>) -> InstrumentProvider {
        Arc::new(SpotInstrumentProvider { dashboard })
    }
}

impl KlineInstrumentProviderTrait for SpotInstrumentProvider {
    fn instrument_type(&self) -> InstrumentType {
        InstrumentType::Spot
    }

    fn instruments(&self, quote_asset: Option<String>) -> Vec<BinanceInstrument> {
        let asset = quote_asset.unwrap_or(USDT_TEXT.to_string());
        self.dashboard
            .spot_living_symbols()
            .into_iter()
            .filter(|inst| inst.quote_asset == asset)
            .collect()
    }

    fn get_instrument(&self, symbol: &str) -> Option<BinanceInstrument> {
        self.dashboard.get_spot_instrument_id_by_symbol(symbol)
    }
}

pub struct SwapInstrumentProvider {
    dashboard: Arc<BinanceDashboard>,
}

impl SwapInstrumentProvider {
    pub fn new_provider(dashboard: Arc<BinanceDashboard>) -> InstrumentProvider {
        Arc::new(SwapInstrumentProvider { dashboard })
    }
}

impl KlineInstrumentProviderTrait for SwapInstrumentProvider {
    fn instrument_type(&self) -> InstrumentType {
        InstrumentType::Swap
    }

    fn instruments(&self, quote_asset: Option<String>) -> Vec<BinanceInstrument> {
        let asset = quote_asset.unwrap_or(USDT_TEXT.to_string());
        self.dashboard
            .swap_living_symbols()
            .into_iter()
            .filter(|inst| inst.quote_asset == asset)
            .collect()
    }

    fn get_instrument(&self, symbol: &str) -> Option<BinanceInstrument> {
        self.dashboard.get_swap_instrument_id_by_symbol(symbol)
    }
}

#[derive(Clone)]
pub struct BinanceDashboardSnapShot {
    pub live_spot: HashMap<String, BinanceInstrument>,
    pub live_swap: HashMap<String, BinanceInstrument>,
    pub timestamp: UnixTimeStamp,
}

impl BinanceDashboardSnapShot {
    pub fn new(live_spot: HashMap<String, BinanceInstrument>, live_swap: HashMap<String, BinanceInstrument>) -> Self {
        Self {
            live_spot,
            live_swap,
            timestamp: unix_time_now_u64_utc(),
        }
    }
}

pub type BinanceDashboardWatcher = watch::Sender<Arc<BinanceDashboardSnapShot>>;

//FUTURE：把这些存入数据库
///
/// 因为Binance所有的instrument都是独立的。所以设立了这个来处理
///
#[derive(Clone)]
pub struct BinanceDashboard {
    spot_symbols: Arc<RwLock<HashMap<String, BinanceInstrument>>>,
    swap_symbols: Arc<RwLock<HashMap<String, BinanceInstrument>>>,
    data_retention_hours: u64,
    debug_mood: bool,
    instrument_repo: BNInstrumentRepository,
}

impl BinanceDashboard {
    pub fn new(data_retention_hours: u64, provider: Option<DuckDBDSProvider>) -> Self {
        BinanceDashboard {
            spot_symbols: Arc::new(RwLock::new(HashMap::new())),
            swap_symbols: Arc::new(RwLock::new(HashMap::new())),
            data_retention_hours,
            debug_mood: false,
            instrument_repo: get_instrument_repo_for_production(provider),
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
                get_instrument_repo_for_production(provider)
            }
            Some(repo) => repo,
        };
        Ok(BinanceDashboard {
            spot_symbols: Arc::new(RwLock::new(HashMap::new())),
            swap_symbols: Arc::new(RwLock::new(HashMap::new())),
            data_retention_hours,
            debug_mood: true,
            instrument_repo: inst_repo,
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

    pub fn read_spot_exchange_info_from_file_sync<P: AsRef<Path>, E: DeserializeOwned>(path: P) -> Result<E, YueError> {
        let s = fs::read_to_string(path)?; // std::io::Error -> YueError::IoError
        let info: E = serde_json::from_str(&s)?; // serde_json::Error -> YueError::SerdeError
        Ok(info)
    }
    pub async fn sync_instruments(&self) -> Result<Arc<BinanceDashboardSnapShot>, YuError> {
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
                // FUTURE: 支持 tradifi perpetual。现在暂时不支持。
                // let tradifi_perpetual = TRADIFI_PERPETUAL_TEXT.to_string();

                let live_spot = self
                    .parse_exchange_symbols(&spot.symbols, vec![InstrumentType::Spot])
                    .await?
                    .into_iter()
                    .filter(|(_, instrument)| matches!(instrument.symbol_type, InstrumentType::Spot) && instrument.status == BN_SYMBOL_STATUS_TRADING)
                    .collect::<HashMap<String, BinanceInstrument>>();
                let live_swap = self
                    .parse_exchange_symbols(
                        &swap.symbols,
                        vec![
                            InstrumentType::Swap,
                            InstrumentType::CurrentQuarter,
                            InstrumentType::TradifiPerpetual,
                            InstrumentType::NextQuarter,
                        ],
                    )
                    .await?
                    .into_iter()
                    .filter(|(_, instrument)| matches!(instrument.symbol_type, InstrumentType::Swap) && instrument.status == BN_SYMBOL_STATUS_TRADING)
                    .collect::<HashMap<String, BinanceInstrument>>();
                {
                    *self.spot_symbols.write().unwrap() = live_spot.clone();
                    *self.swap_symbols.write().unwrap() = live_swap.clone();
                }
                Ok(Arc::new(BinanceDashboardSnapShot::new(live_spot, live_swap)))
            }
            (Err(e), _) => Err(YuError::CustomError(format!("获取现货交易所信息失败: {}", e))),
            (_, Err(e)) => Err(YuError::CustomError(format!("获取永续交易所信息失败: {}", e))),
        }
    }

    ///
    /// # 步骤
    /// 1. 从数据库中，获取symbol和id的hashmap，后续称之symbols。
    /// 2. loop stops。判断symbol是否在存在于symbols.
    ///  - 如果不存在，则转换成po，存入数据库
    ///  - 如果存在，则跳过，在map中删除该条symbol
    /// 3. 把symbol中剩余的数据，标注为非交易
    pub async fn parse_exchange_symbols<S: SymbolInfoTrait>(
        &self,
        symbols: &Vec<S>,
        inst_types: Vec<InstrumentType>,
    ) -> Result<HashMap<String, BinanceInstrument>, YuError> {
        let mut existing_symbols = self.instrument_repo.get_map_of_symbol_id(inst_types).await?;
        let mut live_instruments = HashMap::new();

        info!("exiting xauusdt symbols count: {}", existing_symbols.contains_key("XAUUSDT"));
        for symbol in symbols {
            let instrument = match existing_symbols.remove(symbol.symbol()) {
                Some(server_id) => BinanceInstrument::from_symbol_info_with_id(symbol, server_id as i64),
                None => {
                    info!("exiting xauusdt symbols count: {}", existing_symbols.contains_key("XAUUSDT"));
                    let instrument = BinanceInstrument::from(symbol);
                    if let Err(error) = self.instrument_repo.insert_instrument(instrument.clone()).await {
                        error!(
                            "error adding instrument to symbol {} type is {}: {:?}",
                            symbol.symbol(),
                            symbol.symbol_type(),
                            error
                        );
                        return Err(error);
                    }
                    instrument
                }
            };
            live_instruments.insert(instrument.symbol.clone(), instrument);
        }

        self.instrument_repo
            .mark_instruments_not_trading(existing_symbols.into_values().collect())
            .await?;
        Ok(live_instruments)
    }

    pub fn spot_living_symbols(&self) -> Vec<BinanceInstrument> {
        self.spot_symbols.read().unwrap().values().cloned().collect()
    }

    pub fn get_spot_instrument_id_by_symbol(&self, symbol: &str) -> Option<BinanceInstrument> {
        self.spot_symbols.read().unwrap().get(symbol).cloned()
    }

    pub fn swap_living_symbols(&self) -> Vec<BinanceInstrument> {
        self.swap_symbols.read().unwrap().values().cloned().collect()
    }

    pub fn get_swap_instrument_id_by_symbol(&self, symbol: &str) -> Option<BinanceInstrument> {
        self.swap_symbols.read().unwrap().get(symbol).cloned()
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
