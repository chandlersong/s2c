use serde::Deserialize;
use crate::binance::bn_models::spot_restful::{ExchangeFilter, RateLimit};
use crate::binance::bn_models::swap_restful::SwapExchangeSymbol;

///
///  GET /api/v5/public/instruments
/// {
///             "alias": "",
///             "auctionEndTime": "",
///             "baseCcy": "BTC",
///             "category": "1",
///             "ctMult": "",
///             "ctType": "",
///             "ctVal": "",
///             "ctValCcy": "",
///             "contTdSwTime": "1704876947000",
///             "expTime": "",
///             "futureSettlement": false,
///             "groupId": "1",
///             "instFamily": "",
///             "instId": "BTC-USDT",
///             "instType": "SPOT",
///             "lever": "10",
///             "listTime": "1606468572000",
///             "lotSz": "0.00000001",
///            "maxIcebergSz": "9999999999.0000000000000000",
///             "maxLmtAmt": "1000000",
///             "maxLmtSz": "9999999999",
///             "maxMktAmt": "1000000",
///             "maxMktSz": "",
///             "maxStopSz": "",
///             "maxTriggerSz": "9999999999.0000000000000000",
///             "maxTwapSz": "9999999999.0000000000000000",
///             "minSz": "0.00001",
///             "optType": "",
///             "openType": "call_auction",
///             "preMktSwTime": "",
///             "quoteCcy": "USDT",
///             "tradeQuoteCcyList": [
///                 "USDT"
///             ],
///             "settleCcy": "",
///             "state": "live",
///             "ruleType": "normal",
///            "stk": "",
///             "tickSz": "0.1",
///            "uly": "",
///             "instIdCode": 1000000000,
///             "instCategory": "1",
///             "upcChg": [
///                 {
///                     "param": "tickSz",
///                     "newValue": "0.0001",
///                     "effTime": "1704876947000"
///                 }
///             ]
///         }
///
///
///
use rust_decimal::Decimal;

#[derive(Deserialize, Debug)]
pub struct UpcChgEntry {
    #[serde(rename = "param")]
    /// 变更项的参数名，例如 "tickSz"
    pub param: String,

    #[serde(rename = "newValue", with = "crate::tools::string_to_option_decimal")]
    /// 新值（通常为字符串数字），解析为 Decimal，允许为空
    pub new_value: Option<Decimal>,

    #[serde(rename = "effTime")]
    /// 生效时间（字符串时间戳），保留为字符串以免解析失败
    pub eff_time: Option<String>,
}


#[derive(Deserialize, Debug)]
pub struct InstrumentInfo {
    #[serde(rename = "alias")]
    /// 别名，可能为空字符串
    pub alias: Option<String>,

    #[serde(rename = "auctionEndTime")]
    /// 竞价结束时间（字符串时间戳），可能为空
    pub auction_end_time: Option<String>,

    #[serde(rename = "baseCcy")]
    /// 基础币种，如 "BTC"
    pub base_ccy: String,

    #[serde(rename = "category")]
    /// 分类标识，通常为字符串数字
    pub category: Option<String>,

    #[serde(rename = "ctMult", with = "crate::tools::string_to_option_decimal")]
    /// 合约乘数（字符串），仅对衍生品有，解析为 Decimal
    pub ct_mult: Option<Decimal>,

    #[serde(rename = "ctType")]
    /// 合约类型标识
    pub ct_type: Option<String>,

    #[serde(rename = "ctVal", with = "crate::tools::string_to_option_decimal")]
    /// 合约面值（字符串），解析为 Decimal
    pub ct_val: Option<Decimal>,

    #[serde(rename = "ctValCcy")]
    /// 合约面值币种
    pub ct_val_ccy: Option<String>,

    #[serde(rename = "contTdSwTime")]
    /// 连续交易切换时间（字符串时间戳），可能为空
    pub cont_td_sw_time: Option<String>,

    #[serde(rename = "expTime")]
    /// 到期时间（字符串时间戳），期货/期权适用
    pub exp_time: Option<String>,

    #[serde(rename = "futureSettlement")]
    /// 是否为未来结算，通常为布尔值
    pub future_settlement: Option<bool>,

    #[serde(rename = "groupId")]
    /// 交易对分组ID
    pub group_id: Option<String>,

    #[serde(rename = "instFamily")]
    /// 合约家族标识
    pub inst_family: Option<String>,

    #[serde(rename = "instId")]
    /// 交易品种标识，如 "BTC-USDT"
    pub inst_id: String,

    #[serde(rename = "instType")]
    /// 市场类型，如 "SPOT", "FUTURES"
    pub inst_type: String,

    #[serde(rename = "lever")]
    /// 杠杆，字符串形式（保留字符串，解析策略另行决定）
    pub lever: Option<String>,

    #[serde(rename = "listTime")]
    /// 上线时间（字符串时间戳）
    pub list_time: Option<String>,

    #[serde(rename = "lotSz", with = "crate::tools::string_to_option_decimal")]
    /// 最小交易量，解析为 Decimal
    pub lot_sz: Option<Decimal>,

    #[serde(rename = "maxIcebergSz", with = "crate::tools::string_to_option_decimal")]
    /// 最大冰山订单尺寸，解析为 Decimal
    pub max_iceberg_sz: Option<Decimal>,

    #[serde(rename = "maxLmtAmt", with = "crate::tools::string_to_option_decimal")]
    /// 限价最大金额，解析为 Decimal
    pub max_lmt_amt: Option<Decimal>,

    #[serde(rename = "maxLmtSz", with = "crate::tools::string_to_option_decimal")]
    /// 限价最大数量，解析为 Decimal
    pub max_lmt_sz: Option<Decimal>,

    #[serde(rename = "maxMktAmt", with = "crate::tools::string_to_option_decimal")]
    /// 市价最大金额，解析为 Decimal
    pub max_mkt_amt: Option<Decimal>,

    #[serde(rename = "maxMktSz", with = "crate::tools::string_to_option_decimal")]
    /// 市价最大数量，可能为空，解析为 Decimal
    pub max_mkt_sz: Option<Decimal>,

    #[serde(rename = "maxStopSz", with = "crate::tools::string_to_option_decimal")]
    /// 止损最大数量，可能为空，解析为 Decimal
    pub max_stop_sz: Option<Decimal>,

    #[serde(rename = "maxTriggerSz", with = "crate::tools::string_to_option_decimal")]
    /// 触发单最大数量，解析为 Decimal
    pub max_trigger_sz: Option<Decimal>,

    #[serde(rename = "maxTwapSz", with = "crate::tools::string_to_option_decimal")]
    /// TWAP 最大数量，解析为 Decimal
    pub max_twap_sz: Option<Decimal>,

    #[serde(rename = "minSz", with = "crate::tools::string_to_option_decimal")]
    /// 最小下单数量，解析为 Decimal
    pub min_sz: Option<Decimal>,

    #[serde(rename = "optType")]
    /// 期权类型（如有）
    pub opt_type: Option<String>,

    #[serde(rename = "openType")]
    /// 开盘类型，如 "call_auction"
    pub open_type: Option<String>,

    #[serde(rename = "preMktSwTime")]
    /// 盘前切换时间（字符串时间戳），可能为空
    pub pre_mkt_sw_time: Option<String>,

    #[serde(rename = "quoteCcy")]
    /// 报价币种，如 "USDT"
    pub quote_ccy: Option<String>,

    #[serde(rename = "tradeQuoteCcyList")]
    /// 支持的交易报价币种列表
    pub trade_quote_ccy_list: Option<Vec<String>>,

    #[serde(rename = "settleCcy")]
    /// 结算币种
    pub settle_ccy: Option<String>,

    #[serde(rename = "state")]
    /// 状态，如 "live"
    pub state: Option<String>,

    #[serde(rename = "ruleType")]
    /// 规则类型，如 "normal"
    pub rule_type: Option<String>,

    #[serde(rename = "stk")]
    /// 是否为股票类标识（保留字段）
    pub stk: Option<String>,

    #[serde(rename = "tickSz", with = "crate::tools::string_to_option_decimal")]
    /// 最小价格变动，解析为 Decimal
    pub tick_sz: Option<Decimal>,

    #[serde(rename = "uly")]
    /// 标的合约（Underlying），如期货对应的合约标的
    pub uly: Option<String>,

    #[serde(rename = "instIdCode")]
    /// 内部品种编码（数字，样例为1000000000）
    pub inst_id_code: Option<i64>,

    #[serde(rename = "instCategory")]
    /// 品类标识
    pub inst_category: Option<String>,

    #[serde(rename = "upcChg")]
    /// 属性变更记录数组，解析为结构化对象
    pub upc_chg: Option<Vec<UpcChgEntry>>,
}