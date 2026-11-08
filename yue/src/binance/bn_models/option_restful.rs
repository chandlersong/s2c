use crate::binance::bn_models::spot_restful::RateLimit;
use crate::errors::YueError;
use crate::models::Decimal;
use crate::tools::{string_to_decimal, string_to_option_decimal};
use serde::Deserialize;

//
// URL: /eapi/v1/exchangeInfo

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

impl OptionExchangeInfo {
    pub fn get_request_weight_limit(&self) -> Option<&RateLimit> {
        self.rate_limits.iter().find(|rl| rl.rate_limit_type == "REQUEST_WEIGHT")
    }
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
    #[serde(rename = "nakedSell")]
    pub naked_sell: Option<bool>, // 可选字段，表示是否允许裸卖空
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
    pub naked_sell: Option<bool>,
    #[serde(rename = "status")]
    pub status: String,
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

///
/// /eapi/v1/mark
///
#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct MarketPriceResponse(Vec<OptionMarketPrice>);

impl MarketPriceResponse {
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl FromIterator<OptionMarketPrice> for MarketPriceResponse {
    fn from_iter<T: IntoIterator<Item = OptionMarketPrice>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl IntoIterator for MarketPriceResponse {
    type Item = OptionMarketPrice;
    type IntoIter = std::vec::IntoIter<OptionMarketPrice>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a MarketPriceResponse {
    type Item = &'a OptionMarketPrice;
    type IntoIter = std::slice::Iter<'a, OptionMarketPrice>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::{KLinesResponse, OptionExchangeInfo, OptionMarketPrice};
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
        assert!(!symbol.naked_sell.unwrap_or(false));
        assert_eq!(symbol.status, "TRADING");
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

    // 测试目的：验证期权 K 线响应可以同时反序列化数字和字符串元素。
    // 准备数据/过程：反序列化含字符串价格、数字时间戳和数字价格的 K 线 JSON，并调用公开取值方法。
    // 预期结果：OptionKLine 的各字段均按预期类型读取，响应迭代、长度和兼容访问方法正确。
    #[test]
    fn deserialize_option_klines_with_mixed_value_types() {
        let json = r#"[
            [1499040000000,"0.01","0.02","0.005","0.01634790","12.5",1499644799999,"100.25",7,"6.5","50.75","0"],
            [1499040000001,0.02,0.03,0.01,0.025,10.5,"1499644800000",20.5,9,8.5,19.5,0]
        ]"#;

        let response: KLinesResponse = serde_json::from_str(json).unwrap();
        let kline = response.get(0).unwrap();

        assert_eq!(response.len(), 2);
        assert_eq!(response.iter().count(), 2);
        assert_eq!(kline.candle_begin_time().unwrap(), 1499040000000);
        assert_eq!(kline.open_price().unwrap().to_string(), "0.01");
        assert_eq!(kline.high_price().unwrap().to_string(), "0.02");
        assert_eq!(kline.low_price().unwrap().to_string(), "0.005");
        assert_eq!(kline.close_price().unwrap().to_string(), "0.01634790");
        assert_eq!(kline.close_time().unwrap(), 1499644799999);
        assert_eq!(kline.quote_asset_volume().unwrap().to_string(), "100.25");
        assert_eq!(kline.number_of_trades().unwrap(), 7);
        assert_eq!(kline.taker_buy_base_asset_volume().unwrap().to_string(), "6.5");
        assert_eq!(kline.taker_buy_quote_asset_volume().unwrap().to_string(), "50.75");
        assert_eq!(kline.ignore().unwrap(), &serde_json::Value::String("0".to_string()));
        assert_eq!(response.close_price(0).unwrap().to_string(), "0.01634790");
        assert_eq!(response.volume(0).unwrap().to_string(), "12.5");
        assert_eq!(response.close_time(0).unwrap(), 1499644799999);
        assert_eq!(response.close_price(1).unwrap().to_string(), "0.025");
        assert_eq!(response.volume(1).unwrap().to_string(), "10.5");
        assert_eq!(response.close_time(1).unwrap(), 1499644800000);
    }

    // 测试目的：验证 K 线取值方法遇到缺失索引或错误字段值时返回错误。
    // 准备数据/过程：反序列化收盘价为非数字字符串的记录，并分别读取错误价格和越界记录。
    // 预期结果：close_price 和 volume 均返回 Err，不产生默认值或 panic。
    #[test]
    fn kline_accessors_reject_invalid_values_and_indices() {
        let json = r#"[[1499040000000,"1","2","0","invalid","3",1499644799999]]"#;
        let response: KLinesResponse = serde_json::from_str(json).unwrap();

        assert!(response.close_price(0).is_err());
        assert!(response.volume(1).is_err());
    }

    // 测试目的：验证 KLinesResponse 支持按引用和按值迭代。
    // 准备数据/过程：反序列化两条 K 线，通过引用迭代读取收盘价，再消费响应统计记录数。
    // 预期结果：引用迭代可读取两条收盘价，按值迭代返回两条记录。
    #[test]
    fn kline_response_supports_owned_and_borrowed_iteration() {
        let json = r#"[[0,0,0,0,"1"],[0,0,0,0,"2"]]"#;
        let response: KLinesResponse = serde_json::from_str(json).unwrap();

        let close_prices: Vec<String> = (&response).into_iter().map(|kline| kline.close_price().unwrap().to_string()).collect();
        let owned_count = response.into_iter().count();

        assert_eq!(close_prices, ["1", "2"]);
        assert_eq!(owned_count, 2);
    }
}

///
/// [文档](https://developers.binance.com/zh-CN/docs/catalog/core-trading-derivatives-trading-options/api/rest-api/market-data#kline-candlestick-data)
/// index说明
/// 0: candle_begin_time
/// 1: open price
/// 2: high price
/// 3: low price
/// 4: close price
/// 5: volume
/// 6: close time
/// 7: quote asset volume
/// 8: number of trades
/// 9: taker buy base asset volume
/// 10: taker buy quote asset volume
/// 11: ignore
#[derive(Debug, Deserialize)]
pub struct OptionKLine(Vec<serde_json::Value>);

impl OptionKLine {
    /// Returns the candle begin time in milliseconds.
    pub fn candle_begin_time(&self) -> Result<u64, YueError> {
        self.integer_value(0, "candle begin time")
    }

    /// Returns the open price.
    pub fn open_price(&self) -> Result<Decimal, YueError> {
        self.decimal_value(1, "open price")
    }

    /// Returns the high price.
    pub fn high_price(&self) -> Result<Decimal, YueError> {
        self.decimal_value(2, "high price")
    }

    /// Returns the low price.
    pub fn low_price(&self) -> Result<Decimal, YueError> {
        self.decimal_value(3, "low price")
    }

    /// Returns the close price.
    pub fn close_price(&self) -> Result<Decimal, YueError> {
        self.decimal_value(4, "close price")
    }

    /// Returns the base-asset volume.
    pub fn volume(&self) -> Result<Decimal, YueError> {
        self.decimal_value(5, "volume")
    }

    /// Returns the candle close time in milliseconds.
    pub fn close_time(&self) -> Result<u64, YueError> {
        self.integer_value(6, "close time")
    }

    /// Returns the quote-asset volume.
    pub fn quote_asset_volume(&self) -> Result<Decimal, YueError> {
        self.decimal_value(7, "quote asset volume")
    }

    /// Returns the number of trades.
    pub fn number_of_trades(&self) -> Result<u64, YueError> {
        self.integer_value(8, "number of trades")
    }

    /// Returns the taker-buy base-asset volume.
    pub fn taker_buy_base_asset_volume(&self) -> Result<Decimal, YueError> {
        self.decimal_value(9, "taker buy base asset volume")
    }

    /// Returns the taker-buy quote-asset volume.
    pub fn taker_buy_quote_asset_volume(&self) -> Result<Decimal, YueError> {
        self.decimal_value(10, "taker buy quote asset volume")
    }

    /// Returns the raw value of the reserved ignore field.
    pub fn ignore(&self) -> Result<&serde_json::Value, YueError> {
        self.value(11, "ignore")
    }

    fn decimal_value(&self, index: usize, field: &str) -> Result<Decimal, YueError> {
        let value = self.value(index, field)?;
        let raw = value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string());
        raw.parse()
            .map_err(|error| YueError::ParseError(format!("invalid option kline {field}: {error}")))
    }

    fn integer_value(&self, index: usize, field: &str) -> Result<u64, YueError> {
        let value = self.value(index, field)?;
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|raw| raw.parse().ok()))
            .ok_or_else(|| YueError::ParseError(format!("invalid option kline {field}")))
    }

    fn value(&self, index: usize, field: &str) -> Result<&serde_json::Value, YueError> {
        self.0
            .get(index)
            .ok_or_else(|| YueError::ParseError(format!("option kline is missing {field}")))
    }
}

///
///  URL:/eapi/v1/klines
///
#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct KLinesResponse(Vec<OptionKLine>);

impl KLinesResponse {
    /// Returns the number of K-line records.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the response contains no K-line records.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns an iterator over the K-line records by reference.
    pub fn iter(&self) -> impl Iterator<Item = &OptionKLine> {
        self.0.iter()
    }

    /// Returns the K-line record at `index`.
    pub fn get(&self, index: usize) -> Option<&OptionKLine> {
        self.0.get(index)
    }

    /// Returns the close price for the K-line at `index`.
    pub fn close_price(&self, index: usize) -> Result<Decimal, YueError> {
        self.kline_at(index)?.close_price()
    }

    /// Returns the close time in milliseconds for the K-line at `index`.
    pub fn close_time(&self, index: usize) -> Result<u64, YueError> {
        self.kline_at(index)?.close_time()
    }

    /// Returns the base-asset volume for the K-line at `index`.
    pub fn volume(&self, index: usize) -> Result<Decimal, YueError> {
        self.kline_at(index)?.volume()
    }

    fn kline_at(&self, index: usize) -> Result<&OptionKLine, YueError> {
        self.0
            .get(index)
            .ok_or_else(|| YueError::ParseError(format!("option kline at index {index} does not exist")))
    }
}

impl FromIterator<OptionKLine> for KLinesResponse {
    fn from_iter<T: IntoIterator<Item = OptionKLine>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl IntoIterator for KLinesResponse {
    type Item = OptionKLine;
    type IntoIter = std::vec::IntoIter<OptionKLine>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a KLinesResponse {
    type Item = &'a OptionKLine;
    type IntoIter = std::slice::Iter<'a, OptionKLine>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
