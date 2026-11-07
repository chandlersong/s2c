use crate::binance::bn_models::option_restful::OptionExchangeInfo;
use crate::binance::bn_restful_commands::{OPTION_EXCHANGE_COMMAND, execute_json_request};
use crate::errors::YueError;
use crate::http_client::get_http_client;
use async_trait::async_trait;
use std::sync::{Arc, OnceLock};

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait BinanceOptionApiTrait: Send + Sync {
    async fn get_exchange_info(&self) -> Result<OptionExchangeInfo, YueError>;
}

pub type BinanceOptionExchangeApi = Arc<dyn BinanceOptionApiTrait + Send + Sync>;

pub static SHARE_BN_OPTION_API: OnceLock<BinanceOptionExchangeApi> = OnceLock::new();
pub fn default_binance_option_api() -> BinanceOptionExchangeApi {
    SHARE_BN_OPTION_API.get_or_init(|| BinanceOptionExchangeApiImpl::new()).clone()
}

pub struct BinanceOptionExchangeApiImpl;

impl BinanceOptionExchangeApiImpl {
    pub fn new() -> BinanceOptionExchangeApi {
        Arc::new(BinanceOptionExchangeApiImpl {})
    }
}

#[async_trait]
impl BinanceOptionApiTrait for BinanceOptionExchangeApiImpl {
    async fn get_exchange_info(&self) -> Result<OptionExchangeInfo, YueError> {
        let client = get_http_client();
        let rb = client.get(OPTION_EXCHANGE_COMMAND.as_ref().as_str());
        execute_json_request::<OptionExchangeInfo>(&OPTION_EXCHANGE_COMMAND, rb, None).await
    }
}
