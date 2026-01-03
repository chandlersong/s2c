use crate::binance::bn_models::common::{ExchangeInfoTrait, HistoryVo, SymbolInfoTrait};
use crate::binance::bn_models::spot_restful::{ExchangeFilter, RateLimit};
use crate::tools::{string_to_float, string_to_option_float};
use serde::{Deserialize, Serialize};
/// U本位合约交易所信息结构体
/// 对应于 /fapi/v1/exchangeInfo 接口返回，包含合约市场的全局信息
#[derive(Deserialize, Debug)]
pub struct SwapExchangeInfo {
    #[serde(rename = "timezone")]
    /// 时区，通常为 "UTC"
    pub timezone: String,
    #[serde(rename = "serverTime")]
    /// 服务器当前时间戳（毫秒）
    pub server_time: u64,
    #[serde(rename = "futuresType")]
    /// 合约类型，如 "U_MARGINED"，部分环境可能不存在
    pub futures_type: Option<String>,
    #[serde(rename = "rateLimits")]
    /// 速率限制规则数组
    pub rate_limits: Vec<RateLimit>,
    #[serde(rename = "exchangeFilters")]
    /// 交易所级过滤器数组
    pub exchange_filters: Vec<ExchangeFilter>,
    #[serde(rename = "assets")]
    /// 合约资产信息，结构复杂，通常为资产对象数组
    pub assets: Option<Vec<serde_json::Value>>, // 合约资产信息，结构复杂，先用Value
    #[serde(rename = "symbols")]
    /// 合约交易对信息数组
    pub symbols: Vec<SwapExchangeSymbol>,
}

impl ExchangeInfoTrait for SwapExchangeInfo {
    type SymbolInfo = SwapExchangeSymbol;
    fn timezone(&self) -> &str {
        &self.timezone
    }
    fn server_time(&self) -> u64 {
        self.server_time
    }
    fn symbols(&self) -> &Vec<Self::SymbolInfo> {
        &self.symbols
    }
}

/// U本位合约交易对信息结构体
/// 对应于 /fapi/v1/exchangeInfo 返回的 symbols 数组元素，包含单个合约交易对的详细信息
#[derive(Deserialize, Debug)]
pub struct SwapExchangeSymbol {
    #[serde(rename = "symbol")]
    /// 合约交易对符号，如 "BTCUSDT"
    pub symbol: String,
    #[serde(rename = "pair")]
    /// 标的资产对，如 "BTCUSDT"
    pub pair: String,
    #[serde(rename = "contractType")]
    /// 合约类型，如 "PERPETUAL"、"CURRENT_MONTH"、"NEXT_MONTH"
    pub contract_type: String,
    #[serde(rename = "deliveryDate")]
    /// 交割日期（毫秒时间戳），永续合约为0或无
    pub delivery_date: Option<u64>,
    #[serde(rename = "onboardDate")]
    /// 上线日期（毫秒时间戳）
    pub onboard_date: Option<u64>,
    #[serde(rename = "status")]
    /// 交易状态，如 "TRADING"
    pub status: String,
    #[serde(rename = "maintMarginPercent")]
    /// 维持保证金率（字符串形式）
    pub maint_margin_percent: Option<String>,
    #[serde(rename = "requiredMarginPercent")]
    /// 所需保证金率（字符串形式）
    pub required_margin_percent: Option<String>,
    #[serde(rename = "baseAsset")]
    /// 基础资产，如 "BTC"
    pub base_asset: String,
    #[serde(rename = "quoteAsset")]
    /// 报价资产，如 "USDT"
    pub quote_asset: String,
    #[serde(rename = "marginAsset")]
    /// 保证金资产，如 "USDT"
    pub margin_asset: String,
    #[serde(rename = "pricePrecision")]
    /// 价格精度
    pub price_precision: i32,
    #[serde(rename = "quantityPrecision")]
    /// 数量精度
    pub quantity_precision: i32,
    #[serde(rename = "baseAssetPrecision")]
    /// 基础资产精度
    pub base_asset_precision: i32,
    #[serde(rename = "quotePrecision")]
    /// 报价资产精度
    pub quote_precision: i32,
    #[serde(rename = "underlyingType")]
    /// 标的类型（如 "COIN"），部分合约有
    pub underlying_type: Option<String>,
    #[serde(rename = "underlyingSubType")]
    /// 标的子类型数组，部分合约有
    pub underlying_sub_type: Option<Vec<String>>,
    #[serde(rename = "settlePlan")]
    /// 结算计划，部分合约有
    pub settle_plan: Option<i32>,
    #[serde(rename = "triggerProtect")]
    /// 触发保护价格，部分合约有
    pub trigger_protect: Option<String>,
    #[serde(rename = "filters")]
    /// 交易对级过滤器数组
    pub filters: Vec<ExchangeFilter>,
    #[serde(rename = "orderTypes")]
    /// 支持的订单类型数组
    pub order_types: Vec<String>,
    #[serde(rename = "timeInForce")]
    /// 支持的时效类型数组，部分合约有
    pub time_in_force: Option<Vec<String>>,
    #[serde(rename = "liquidationFee")]
    /// 强平手续费率，部分合约有
    pub liquidation_fee: Option<String>,
    #[serde(rename = "marketTakeBound")]
    /// 市价单吃单限制，部分合约有
    pub market_take_bound: Option<String>,
    #[serde(rename = "maxMoveOrderLimit")]
    /// 最大移动下单限制，部分合约有
    pub max_move_order_limit: Option<i32>,
    #[serde(rename = "priceScale")]
    /// 价格刻度，部分合约有
    pub price_scale: Option<i32>,
    // 其他合约特有字段可按需补充
}

impl SymbolInfoTrait for SwapExchangeSymbol {
    fn symbol(&self) -> &str {
        &self.symbol
    }
    fn status(&self) -> &str {
        &self.status
    }
    fn base_asset(&self) -> &str {
        &self.base_asset
    }
    fn quote_asset(&self) -> &str {
        &self.quote_asset
    }
    fn order_types(&self) -> &Vec<String> {
        &self.order_types
    }
    fn quote_precision(&self) -> i32 {
        self.quote_precision
    }
    fn symbol_type(&self) -> &str {
        &self.contract_type
    }
    fn get_on_board_time(&self) -> Option<u64> {
        self.onboard_date
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingRate {
    #[serde(rename = "symbol")]
    pub symbol: String,

    #[serde(rename = "fundingRate")]
    #[serde(with = "string_to_float")]
    pub funding_rate: f64,

    #[serde(rename = "fundingTime")]
    pub funding_time: u64,

    #[serde(rename = "markPrice")]
    #[serde(with = "string_to_option_float")]
    pub mark_price: Option<f64>, // 资金费对应标记价格，允许为空
}

impl HistoryVo for FundingRate {
    fn get_close_time(&self) -> u64 {
        self.funding_time
    }
    fn get_open_time(&self) -> u64 {
        self.funding_time
    }
}
