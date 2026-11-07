use crate::binance::bn_models::common::SymbolInfoTrait;
use crate::binance::bn_models::spot_restful::RateLimit;
use crate::models::Decimal;
use crate::tools::{string_to_decimal, string_to_option_decimal};
use serde::Deserialize;

//
// {
//   "timezone": "UTC",
//   "serverTime": 1592387337630,
//   "optionContracts": [
//     {
//       "baseAsset": "BTC",
//       "quoteAsset": "USDT",
//       "underlying": "BTCUSDT",
//       "settleAsset": "USDT"
//     }
//   ],
//   "optionAssets": [
//     {
//       "name": "USDT"
//     }
//   ],
//   "optionSymbols": [
//     {
//       "expiryDate": 1660521600000,
//       "filters": [
//         {
//           "filterType": "PRICE_FILTER",
//           "minPrice": "0.02",
//           "maxPrice": "80000.01",
//           "tickSize": "0.01",
//           "minQty": "0.01",
//           "maxQty": "100",
//           "stepSize": "0.01"
//         }
//       ],
//       "symbol": "BTC-220815-50000-C",
//       "side": "CALL",
//       "strikePrice": "50000",
//       "underlying": "BTCUSDT",
//       "unit": 1,
//       "liquidationFeeRate": "0.0019000",
//       "minQty": "0.01",
//       "maxQty": "100",
//       "initialMargin": "0.15",
//       "maintenanceMargin": "0.075",
//       "minInitialMargin": "0.1",
//       "minMaintenanceMargin": "0.05",
//       "priceScale": 2,
//       "quantityScale": 2,
//       "quoteAsset": "USDT",
//       "contractType": "CRYPTO_OPTIONS",
//       "underlyingType": "CRYPTO",
//       "nakedSell": false,
//       "status": "TRADING"
//     }
//   ],
//   "rateLimits": [
//     {
//       "rateLimitType": "REQUEST_WEIGHT",
//       "interval": "MINUTE",
//       "intervalNum": 1,
//       "limit": 2400
//     }
//   ]
// }
#[derive(Debug, Deserialize)]
pub struct OptionExchangeInfo {
    #[serde(rename = "timezone")]
    pub timezone: String,
    #[serde(rename = "serverTime")]
    pub server_time: u64,
    #[serde(rename = "optionContracts")]
    pub option_contracts: Vec<OptionContract>,
    #[serde(rename = "optionAssets")]
    pub option_assets: Vec<OptionAsset>,
    #[serde(rename = "optionSymbols")]
    pub option_symbols: Vec<OptionSymbol>,
    #[serde(rename = "rateLimits")]
    pub rate_limits: Vec<RateLimit>,
}

#[derive(Debug, Deserialize)]
pub struct OptionContract {
    #[serde(rename = "baseAsset")]
    pub base_asset: String,
    #[serde(rename = "quoteAsset")]
    pub quote_asset: String,
    #[serde(rename = "underlying")]
    pub underlying: String,
    #[serde(rename = "settleAsset")]
    pub settle_asset: String,
}

#[derive(Debug, Deserialize)]
pub struct OptionAsset {
    #[serde(rename = "name")]
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct OptionSymbol {
    #[serde(rename = "expiryDate")]
    pub expiry_date: u64,
    #[serde(rename = "filters")]
    pub filters: Vec<OptionSymbolFilter>,
    #[serde(rename = "symbol")]
    pub symbol: String,
    #[serde(rename = "side")]
    pub side: String,
    #[serde(rename = "strikePrice", with = "string_to_decimal")]
    pub strike_price: Decimal,
    #[serde(rename = "underlying")]
    pub underlying: String,
    #[serde(rename = "unit")]
    pub unit: i32,
    #[serde(rename = "liquidationFeeRate", with = "string_to_decimal")]
    pub liquidation_fee_rate: Decimal,
    #[serde(rename = "minQty", with = "string_to_decimal")]
    pub min_qty: Decimal,
    #[serde(rename = "maxQty", with = "string_to_decimal")]
    pub max_qty: Decimal,
    #[serde(rename = "initialMargin", with = "string_to_decimal")]
    pub initial_margin: Decimal,
    #[serde(rename = "maintenanceMargin", with = "string_to_decimal")]
    pub maintenance_margin: Decimal,
    #[serde(rename = "minInitialMargin", with = "string_to_decimal")]
    pub min_initial_margin: Decimal,
    #[serde(rename = "minMaintenanceMargin", with = "string_to_decimal")]
    pub min_maintenance_margin: Decimal,
    #[serde(rename = "priceScale")]
    pub price_scale: i32,
    #[serde(rename = "quantityScale")]
    pub quantity_scale: i32,
    #[serde(rename = "quoteAsset")]
    pub quote_asset: String,
    #[serde(rename = "contractType")]
    pub contract_type: String,
    #[serde(rename = "underlyingType")]
    pub underlying_type: String,
    #[serde(rename = "nakedSell")]
    pub naked_sell: bool,
    #[serde(rename = "status")]
    pub status: String,
}

impl SymbolInfoTrait for OptionSymbol {
    fn symbol(&self) -> &str {
        &self.symbol
    }

    fn status(&self) -> &str {
        &self.status
    }

    fn base_asset(&self) -> &str {
        self.underlying.strip_suffix(&self.quote_asset).unwrap_or(&self.underlying)
    }

    fn quote_asset(&self) -> &str {
        &self.quote_asset
    }

    fn order_types(&self) -> &Vec<String> {
        static EMPTY_ORDER_TYPES: Vec<String> = Vec::new();
        &EMPTY_ORDER_TYPES
    }

    fn quote_precision(&self) -> i32 {
        self.price_scale
    }

    fn symbol_type(&self) -> &str {
        &self.contract_type
    }
}

#[derive(Debug, Deserialize)]
pub struct OptionSymbolFilter {
    #[serde(rename = "filterType")]
    pub filter_type: String,
    #[serde(rename = "minPrice", default, deserialize_with = "string_to_option_decimal::deserialize")]
    pub min_price: Option<Decimal>,
    #[serde(rename = "maxPrice", default, deserialize_with = "string_to_option_decimal::deserialize")]
    pub max_price: Option<Decimal>,
    #[serde(rename = "tickSize", default, deserialize_with = "string_to_option_decimal::deserialize")]
    pub tick_size: Option<Decimal>,
    #[serde(rename = "minQty", default, deserialize_with = "string_to_option_decimal::deserialize")]
    pub min_qty: Option<Decimal>,
    #[serde(rename = "maxQty", default, deserialize_with = "string_to_option_decimal::deserialize")]
    pub max_qty: Option<Decimal>,
    #[serde(rename = "stepSize", default, deserialize_with = "string_to_option_decimal::deserialize")]
    pub step_size: Option<Decimal>,
}

// [
//   {
//     "symbol": "BTC-200730-9000-C",
//     "markPrice": "1343.2883",
//     "bidIV": "1.40000077",
//     "askIV": "1.50000153",
//     "markIV": "1.45000000",
//     "delta": "0.55937056",
//     "theta": "3739.82509871",
//     "gamma": "0.00010969",
//     "vega": "978.58874732",
//     "highPriceLimit": "1618.241",
//     "lowPriceLimit": "1068.3356",
//     "riskFreeInterest": "0.1"
//   }
// ]
#[derive(Debug, Deserialize)]
pub struct OptionMarketPrice {
    #[serde(rename = "symbol")]
    pub symbol: String,
    #[serde(rename = "markPrice", with = "string_to_decimal")]
    pub mark_price: Decimal,
    #[serde(rename = "bidIV", with = "string_to_decimal")]
    pub bid_iv: Decimal,
    #[serde(rename = "askIV", with = "string_to_decimal")]
    pub ask_iv: Decimal,
    #[serde(rename = "markIV", with = "string_to_decimal")]
    pub mark_iv: Decimal,
    #[serde(rename = "delta", with = "string_to_decimal")]
    pub delta: Decimal,
    #[serde(rename = "theta", with = "string_to_decimal")]
    pub theta: Decimal,
    #[serde(rename = "gamma", with = "string_to_decimal")]
    pub gamma: Decimal,
    #[serde(rename = "vega", with = "string_to_decimal")]
    pub vega: Decimal,
    #[serde(rename = "highPriceLimit", with = "string_to_decimal")]
    pub high_price_limit: Decimal,
    #[serde(rename = "lowPriceLimit", with = "string_to_decimal")]
    pub low_price_limit: Decimal,
    #[serde(rename = "riskFreeInterest", with = "string_to_decimal")]
    pub risk_free_interest: Decimal,
}

#[cfg(test)]
mod tests {
    use super::{OptionExchangeInfo, OptionMarketPrice};
    use crate::binance::bn_models::common::SymbolInfoTrait;
    use crate::models::Decimal;

    // 测试目的：验证期权交易所信息字段映射、Decimal 转换，以及 OptionSymbol 的 SymbolInfoTrait 映射。
    // 准备数据/过程：构造包含合约、资产、期权合约、过滤器和速率限制的 JSON，再反序列化为 OptionExchangeInfo。
    // 预期结果：嵌套文本、时间、Decimal 属性和交易对通用属性均正确，未提供的可选过滤器属性为 None。
    #[test]
    fn deserialize_option_exchange_info() {
        let json = r#"{
            "timezone":"UTC",
            "serverTime":1592387337630,
            "optionContracts":[{"baseAsset":"BTC","quoteAsset":"USDT","underlying":"BTCUSDT","settleAsset":"USDT"}],
            "optionAssets":[{"name":"USDT"}],
            "optionSymbols":[{
                "expiryDate":1660521600000,
                "filters":[
                    {"filterType":"PRICE_FILTER","minPrice":"0.02","maxPrice":"80000.01","tickSize":"0.01"},
                    {"filterType":"LOT_SIZE","minQty":"0.01","maxQty":"100","stepSize":"0.01"}
                ],
                "symbol":"BTC-220815-50000-C",
                "side":"CALL",
                "strikePrice":"50000",
                "underlying":"BTCUSDT",
                "unit":1,
                "liquidationFeeRate":"0.0019000",
                "minQty":"0.01",
                "maxQty":"100",
                "initialMargin":"0.15",
                "maintenanceMargin":"0.075",
                "minInitialMargin":"0.1",
                "minMaintenanceMargin":"0.05",
                "priceScale":2,
                "quantityScale":2,
                "quoteAsset":"USDT",
                "contractType":"CRYPTO_OPTIONS",
                "underlyingType":"CRYPTO",
                "nakedSell":false,
                "status":"TRADING"
            }],
            "rateLimits":[{"rateLimitType":"REQUEST_WEIGHT","interval":"MINUTE","intervalNum":1,"limit":2400}]
        }"#;

        let info: OptionExchangeInfo = serde_json::from_str(json).unwrap();
        let symbol = &info.option_symbols[0];

        assert_eq!(info.timezone, "UTC");
        assert_eq!(info.server_time, 1592387337630);
        assert_eq!(info.option_contracts[0].base_asset, "BTC");
        assert_eq!(info.option_contracts[0].quote_asset, "USDT");
        assert_eq!(info.option_contracts[0].underlying, "BTCUSDT");
        assert_eq!(info.option_contracts[0].settle_asset, "USDT");
        assert_eq!(info.option_assets[0].name, "USDT");
        assert_eq!(symbol.symbol, "BTC-220815-50000-C");
        assert_eq!(symbol.expiry_date, 1660521600000);
        assert_eq!(symbol.side, "CALL");
        assert_eq!(symbol.strike_price, Decimal::new(50000, 0));
        assert_eq!(symbol.underlying, "BTCUSDT");
        assert_eq!(symbol.unit, 1);
        assert_eq!(symbol.liquidation_fee_rate, Decimal::new(19, 4));
        assert_eq!(symbol.min_qty, Decimal::new(1, 2));
        assert_eq!(symbol.max_qty, Decimal::new(100, 0));
        assert_eq!(symbol.initial_margin, Decimal::new(15, 2));
        assert_eq!(symbol.maintenance_margin, Decimal::new(75, 3));
        assert_eq!(symbol.min_initial_margin, Decimal::new(1, 1));
        assert_eq!(symbol.min_maintenance_margin, Decimal::new(5, 2));
        assert_eq!(symbol.price_scale, 2);
        assert_eq!(symbol.quantity_scale, 2);
        assert_eq!(symbol.quote_asset, "USDT");
        assert_eq!(symbol.contract_type, "CRYPTO_OPTIONS");
        assert_eq!(symbol.underlying_type, "CRYPTO");
        assert!(!symbol.naked_sell);
        assert_eq!(symbol.status, "TRADING");
        assert_eq!(symbol.symbol(), "BTC-220815-50000-C");
        assert_eq!(symbol.status(), "TRADING");
        assert_eq!(symbol.base_asset(), "BTC");
        assert_eq!(symbol.quote_asset(), "USDT");
        assert!(symbol.order_types().is_empty());
        assert_eq!(symbol.quote_precision(), 2);
        assert_eq!(symbol.symbol_type(), "CRYPTO_OPTIONS");
        assert_eq!(symbol.get_on_board_time(), None);
        assert_eq!(symbol.filters[0].filter_type, "PRICE_FILTER");
        assert_eq!(symbol.filters[0].min_price, Some(Decimal::new(2, 2)));
        assert_eq!(symbol.filters[0].max_price, Some(Decimal::new(8000001, 2)));
        assert_eq!(symbol.filters[0].tick_size, Some(Decimal::new(1, 2)));
        assert_eq!(symbol.filters[0].min_qty, None);
        assert_eq!(symbol.filters[1].min_price, None);
        assert_eq!(symbol.filters[1].min_qty, Some(Decimal::new(1, 2)));
        assert_eq!(symbol.filters[1].max_qty, Some(Decimal::new(100, 0)));
        assert_eq!(symbol.filters[1].step_size, Some(Decimal::new(1, 2)));
        assert_eq!(info.rate_limits[0].rate_limit_type, "REQUEST_WEIGHT");
        assert_eq!(info.rate_limits[0].interval, "MINUTE");
        assert_eq!(info.rate_limits[0].interval_num, 1);
        assert_eq!(info.rate_limits[0].limit, 2400);
    }

    // 测试目的：验证期权市场价格响应中各价格、波动率及希腊值属性的字段映射和 Decimal 解析。
    // 准备数据/过程：将注释中的市场价格 JSON 对象反序列化为 OptionMarketPrice。
    // 预期结果：符号与每个数值属性都能正确读取，且 Decimal 保留小数精度。
    #[test]
    fn deserialize_option_market_price() {
        let json = r#"{
            "symbol":"BTC-200730-9000-C",
            "markPrice":"1343.2883",
            "bidIV":"1.40000077",
            "askIV":"1.50000153",
            "markIV":"1.45000000",
            "delta":"0.55937056",
            "theta":"3739.82509871",
            "gamma":"0.00010969",
            "vega":"978.58874732",
            "highPriceLimit":"1618.241",
            "lowPriceLimit":"1068.3356",
            "riskFreeInterest":"0.1"
        }"#;

        let price: OptionMarketPrice = serde_json::from_str(json).unwrap();

        assert_eq!(price.symbol, "BTC-200730-9000-C");
        assert_eq!(price.mark_price, Decimal::new(13432883, 4));
        assert_eq!(price.bid_iv, Decimal::new(140000077, 8));
        assert_eq!(price.ask_iv, Decimal::new(150000153, 8));
        assert_eq!(price.mark_iv, Decimal::new(145000000, 8));
        assert_eq!(price.delta, Decimal::new(55937056, 8));
        assert_eq!(price.theta, Decimal::new(373982509871, 8));
        assert_eq!(price.gamma, Decimal::new(10969, 8));
        assert_eq!(price.vega, Decimal::new(97858874732, 8));
        assert_eq!(price.high_price_limit, Decimal::new(1618241, 3));
        assert_eq!(price.low_price_limit, Decimal::new(10683356, 4));
        assert_eq!(price.risk_free_interest, Decimal::new(1, 1));
    }

    // 测试目的：验证格式错误的期权数值不会被静默接受。
    // 准备数据/过程：将包含非法 markPrice 字符串的市场价格 JSON 反序列化。
    // 预期结果：反序列化返回错误。
    #[test]
    fn rejects_invalid_option_market_price_decimal() {
        let json = r#"{
            "symbol":"BTC-200730-9000-C",
            "markPrice":"invalid",
            "bidIV":"1.4",
            "askIV":"1.5",
            "markIV":"1.45",
            "delta":"0.5",
            "theta":"1",
            "gamma":"0.1",
            "vega":"1",
            "highPriceLimit":"2",
            "lowPriceLimit":"1",
            "riskFreeInterest":"0.1"
        }"#;

        assert!(serde_json::from_str::<OptionMarketPrice>(json).is_err());
    }
}
