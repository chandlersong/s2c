use bon::Builder;
use rust_decimal::Decimal;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone, Builder)]
pub struct OkxListResponse<T: Clone> {
    pub code: String,
    pub msg: String,
    pub data: Vec<T>,
}

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

#[derive(Deserialize, Debug, Clone, Builder)]
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

#[derive(Deserialize, Debug, Clone, Builder)]
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

    #[serde(rename = "expTime", with = "crate::tools::string_to_option_u64")]
    /// 到期时间（字符串时间戳），期货/期权适用
    pub exp_time: Option<u64>,

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

    #[serde(rename = "listTime", with = "crate::tools::string_to_option_u64")]
    /// 上线时间（字符串时间戳）
    pub list_time: Option<u64>,

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

///
///  /api/v5/market/history-candles
///  /api/v5/market/candles
///. 因为这些都是一些list。所以暂时就这么处理了。
///  [字段顺序](https://www.okx.com/docs-v5/zh/#order-book-trading-market-data-get-candlesticks)
///
pub type CandleResponse = OkxListResponse<Vec<String>>;

///
///       {
///             "askVol": "3.7207056835937498",
///             "bidVol": "0",
///             "delta": "0.8310206676289528",
///             "deltaBS": "0.9857332101544538",
///             "fwdPx": "39016.8143629068452065",
///             "gamma": "-1.1965483553276135",
///             "gammaBS": "0.000011933182397798109",
///             "instId": "BTC-USD-220309-33000-C",
///             "instType": "OPTION",
///             "lever": "0",
///             "markVol": "1.5551965233045728",
///             "realVol": "0",
///             "volLv": "0",
///             "theta": "-0.0014131955002093717",
///             "thetaBS": "-66.03526900575946",
///             "ts": "1646733631242",
///             "uly": "BTC-USD",
///             "vega": "0.000018173851073258973",
///             "vegaBS": "0.7089307622132419"
///         }
#[derive(Deserialize, Debug, Clone, Builder)]
pub struct OptionSummaryDetail {
    #[serde(rename = "askVol", with = "crate::tools::string_to_option_decimal")]
    /// ask 波动率
    pub ask_vol: Option<Decimal>,

    #[serde(rename = "bidVol", with = "crate::tools::string_to_option_decimal")]
    /// bid 波动率
    pub bid_vol: Option<Decimal>,

    #[serde(rename = "delta", with = "crate::tools::string_to_option_decimal")]
    /// 期权价格对 uly 价格的敏感度
    pub delta: Option<Decimal>,

    #[serde(rename = "deltaBS", with = "crate::tools::string_to_option_decimal")]
    /// BS 模式下期权价格对 uly 价格的敏感度
    pub delta_bs: Option<Decimal>,

    #[serde(rename = "fwdPx", with = "crate::tools::string_to_option_decimal")]
    /// 远期价格
    pub fwd_px: Option<Decimal>,

    #[serde(rename = "gamma", with = "crate::tools::string_to_option_decimal")]
    /// delta 对 uly 价格的敏感度
    pub gamma: Option<Decimal>,

    #[serde(rename = "gammaBS", with = "crate::tools::string_to_option_decimal")]
    /// BS 模式下 delta 对 uly 价格的敏感度
    pub gamma_bs: Option<Decimal>,

    #[serde(rename = "instId")]
    /// 产品 ID，如 BTC-USD-200103-5500-C
    pub inst_id: String,

    #[serde(rename = "instType")]
    /// 产品类型，OPTION：期权
    pub inst_type: String,

    #[serde(rename = "lever", with = "crate::tools::string_to_option_decimal")]
    /// 杠杆倍数
    pub lever: Option<Decimal>,

    #[serde(rename = "markVol", with = "crate::tools::string_to_option_decimal")]
    /// 标记波动率
    pub mark_vol: Option<Decimal>,

    #[serde(rename = "realVol", with = "crate::tools::string_to_option_decimal")]
    /// 已实现波动率（目前该字段暂未启用）
    pub real_vol: Option<Decimal>,

    #[serde(rename = "volLv", with = "crate::tools::string_to_option_decimal")]
    /// 平价期权的隐含波动率
    pub vol_lv: Option<Decimal>,

    #[serde(rename = "theta", with = "crate::tools::string_to_option_decimal")]
    /// 期权价格对剩余期限的敏感度
    pub theta: Option<Decimal>,

    #[serde(rename = "thetaBS", with = "crate::tools::string_to_option_decimal")]
    /// BS 模式下期权价格对剩余期限的敏感度
    pub theta_bs: Option<Decimal>,

    #[serde(rename = "ts", with = "crate::tools::string_to_option_u64")]
    /// 数据更新时间，Unix 时间戳的毫秒数，如 1597026383085
    pub ts: Option<u64>,

    #[serde(rename = "uly")]
    /// 标的指数
    pub uly: Option<String>,

    #[serde(rename = "vega", with = "crate::tools::string_to_option_decimal")]
    /// 期权价格对隐含波动率的敏感度
    pub vega: Option<Decimal>,

    #[serde(rename = "vegaBS", with = "crate::tools::string_to_option_decimal")]
    /// BS 模式下期权价格对隐含波动率的敏感度
    pub vega_bs: Option<Decimal>,
}

///
/// [获取期权定价](https://www.okx.com/docs-v5/zh/#public-data-rest-api-get-option-market-data)
/// GET /api/v5/public/opt-summary
pub type OptionSummaryResponse = OkxListResponse<Vec<OptionSummaryDetail>>;
