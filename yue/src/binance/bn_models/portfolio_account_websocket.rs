//! [统一账户stream对象](https://developers.binance.com/docs/zh-CN/derivatives/portfolio-margin/user-data-streams/Event-Conditional-Order-Trade-Update)

use crate::models::Decimal;
use crate::tools::{string_to_decimal, string_to_option_decimal};
use actix::Message;
use li::errors::LiError;
use li::websocket::models::WebSocketMessage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 统一账户信息的websocket推送
#[derive(Debug, Deserialize, Serialize, Clone, Message)]
#[rtype(result = "()")]
#[serde(untagged)]
pub enum BinancePortfolioWebSocketStreamResponse {
    ConditionalOrderTradeUpdate(ConditionalOrderTradeUpdatePayload),
    OpenOrderLoss(OpenOrderLossPayload),
    OutboundAccountPosition(OutboundAccountPositionPayload),
    LiabilityChange(LiabilityChangePayload),
    ExecutionReport(ExecutionReportPayload),
    OrderTradeUpdate(OrderTradeUpdatePayload),
    AccountUpdate(AccountUpdatePayload),
    AccountConfigUpdate(AccountConfigUpdatePayload),
    RiskLevelChange(RiskLevelChangePayload),
    BalanceUpdate(BalanceUpdatePayload),
    UnKnow(Value),
}

impl WebSocketMessage for BinancePortfolioWebSocketStreamResponse {
    fn from_text(text: &str) -> Result<Self, LiError> {
        serde_json::from_str(text).map_err(|e| LiError::from(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_conditional_order_trade_update() {
        let json = r#"{
            "e":"CONDITIONAL_ORDER_TRADE_UPDATE",
            "T":1669262908216,
            "E":1669262908218,
            "fs":"UM",
            "so":{
                "s":"BTCUSDT",
                "c":"TEST",
                "si":176057039,
                "S":"SELL",
                "st":"TRAILING_STOP_MARKET",
                "f":"GTC",
                "q":"0.001",
                "p":"0",
                "sp":"7103.04",
                "os":"NEW",
                "T":1568879465650,
                "ut":1669262908216,
                "R":false,
                "wt":"MARK_PRICE",
                "ps":"LONG",
                "cp":false,
                "AP":"7476.89",
                "cr":"5.0",
                "i":8886774,
                "V":"EXPIRE_TAKER",
                "gtd":0
            }
        }"#;

        let p: ConditionalOrderTradeUpdatePayload = serde_json::from_str(json).expect("should parse conditional update");
        assert_eq!(p.event, "CONDITIONAL_ORDER_TRADE_UPDATE");
        assert_eq!(p.trade_time, 1669262908216);
        assert_eq!(p.business, "UM");
        let so = p.strategy.expect("strategy present");
        assert_eq!(so.symbol, "BTCUSDT");
        assert_eq!(so.order_id, 8886774);
        assert_eq!(so.activation_price.to_string(), "7476.89");
    }

    #[test]
    fn parse_execution_report() {
        let json = r#"{
            "e":"executionReport",
            "E":1499405658658,
            "s":"ETHBTC",
            "c":"mUvoqJxFIILMdfAW5iGSOW",
            "S":"BUY",
            "o":"LIMIT",
            "f":"GTC",
            "q":"1.00000000",
            "p":"0.10264410",
            "P":"0.00000000",
            "F":"0.00000000",
            "g":-1,
            "C":"",
            "x":"NEW",
            "X":"NEW",
            "r":"NONE",
            "i":4293153,
            "l":"0.00000000",
            "z":"0.00000000",
            "L":"0.00000000",
            "n":"0",
            "N":null,
            "T":1499405658657,
            "t":-1,
            "v":3,
            "I":8641984,
            "w":true,
            "m":false,
            "O":1499405658657,
            "Z":"0.00000000",
            "Y":"0.00000000",
            "Q":"0.00000000",
            "W":1499405658657,
            "V":"NONE"
        }"#;

        let p: ExecutionReportPayload = serde_json::from_str(json).expect("should parse execution report");
        assert_eq!(p.event, "executionReport");
        assert_eq!(p.event_time, 1499405658658);
        assert_eq!(p.symbol, "ETHBTC");
        assert_eq!(p.order_id, 4293153);
        assert_eq!(p.order_status, "NEW");
    }

    #[test]
    fn parse_open_order_loss() {
        let json = r#"{
            "e":"openOrderLoss",
            "E":1678710578788,
            "O":[{"a":"BUSD","o":"-0.1232313"}]
        }"#;

        let p: OpenOrderLossPayload = serde_json::from_str(json).expect("should parse open order loss");
        assert_eq!(p.event, "openOrderLoss");
        assert_eq!(p.event_time, 1678710578788);
        let items = p.updates.expect("updates present");
        assert_eq!(items[0].asset, "BUSD");
    }

    #[test]
    fn parse_outbound_account_position() {
        let json = r#"{
            "e":"outboundAccountPosition",
            "E":1564034571105,
            "u":1564034571073,
            "U":1027053479517,
            "B":[{"a":"ETH","f":"10000.000000","l":"0.000000"}]
        }"#;

        let p: OutboundAccountPositionPayload = serde_json::from_str(json).expect("should parse outbound account position");
        assert_eq!(p.event, "outboundAccountPosition");
        assert_eq!(p.event_time, 1564034571105);
        let bals = p.balances.expect("balances present");
        assert_eq!(bals[0].asset, "ETH");
    }

    #[test]
    fn parse_liability_change() {
        let json = r#"{
            "e":"liabilityChange",
            "E":1573200697110,
            "a":"BTC",
            "t":"BORROW",
            "T":1352286576452864727,
            "p":"1.03453430",
            "i":"0",
            "l":"1.03476851"
        }"#;

        let p: LiabilityChangePayload = serde_json::from_str(json).expect("should parse liability change");
        assert_eq!(p.event, "liabilityChange");
        assert_eq!(p.asset, "BTC");
        assert_eq!(p.change_type, "BORROW");
        assert_eq!(p.total_liability.to_string(), "1.03476851");
    }

    #[test]
    fn parse_order_trade_update_variant() {
        let json = r#"{
            "e":"ORDER_TRADE_UPDATE",
            "E":1568879465651,
            "T":1568879465650,
            "fs":"UM",
            "o":{
                "s":"BTCUSDT",
                "c":"TEST",
                "S":"SELL",
                "o":"TRAILING_STOP_MARKET",
                "f":"GTC",
                "q":"0.001",
                "p":"0",
                "ap":"0",
                "sp":"0",
                "x":"NEW",
                "X":"NEW",
                "i":8886774,
                "l":"0",
                "z":"0",
                "L":"0",
                "N":"USDT",
                "n":"0",
                "T":1568879465650,
                "t":0,
                "b":"0",
                "a":"0",
                "R":false,
                "ps":"LONG",
                "rp":"0",
                "si":12893,
                "v":"EXPIRE_TAKER",
                "gtd":0
            }
        }"#;

        let p: OrderTradeUpdatePayload = serde_json::from_str(json).expect("should parse order trade update");
        assert_eq!(p.event, "ORDER_TRADE_UPDATE");
        assert_eq!(p.business, "UM");
        let o = p.order;
        assert_eq!(o.symbol, "BTCUSDT");
        assert_eq!(o.order_id, 8886774);
    }

    #[test]
    fn parse_account_update() {
        let json = r#"{
            "e":"ACCOUNT_UPDATE",
            "fs":"UM",
            "E":1564745798939,
            "T":1564745798938,
            "i":"",
            "a": { "m":"ORDER" }
        }"#;

        let p: AccountUpdatePayload = serde_json::from_str(json).expect("should parse account update");
        assert_eq!(p.event, "ACCOUNT_UPDATE");
        assert_eq!(p.business, "UM");
        let acct = p.account.expect("account present");
        assert_eq!(acct.reason, "ORDER");
    }

    #[test]
    fn parse_account_config_update() {
        let json = r#"{
            "e":"ACCOUNT_CONFIG_UPDATE",
            "fs":"UM",
            "E":1611646737479,
            "T":1611646737476,
            "ac": { "s":"BTCUSD_PERP", "l":25 }
        }"#;

        let p: AccountConfigUpdatePayload = serde_json::from_str(json).expect("should parse account config update");
        assert_eq!(p.event, "ACCOUNT_CONFIG_UPDATE");
        assert_eq!(p.business, "UM");
        let ac = p.ac.expect("ac present");
        assert_eq!(ac.symbol, "BTCUSD_PERP");
        assert_eq!(ac.leverage, 25);
    }

    #[test]
    fn parse_risk_level_change() {
        let json = r#"{
            "e":"riskLevelChange",
            "E":1587727187525,
            "u":"1.99999999",
            "s":"MARGIN_CALL",
            "eq":"30.23416728",
            "ae":"30.23416728",
            "m":"15.11708371"
        }"#;

        let p: RiskLevelChangePayload = serde_json::from_str(json).expect("should parse risk level change");
        assert_eq!(p.event, "riskLevelChange");
        assert_eq!(p.status, "MARGIN_CALL");
        assert_eq!(p.equity.to_string(), "30.23416728");
    }

    #[test]
    fn parse_balance_update() {
        let json = r#"{
            "e":"balanceUpdate",
            "E":1573200697110,
            "a":"BTC",
            "d":"100.00000000",
            "U":1027053479517,
            "T":1573200697068
        }"#;

        let p: BalanceUpdatePayload = serde_json::from_str(json).expect("should parse balance update");
        assert_eq!(p.event, "balanceUpdate");
        assert_eq!(p.asset, "BTC");
        assert_eq!(p.delta.to_string(), "100.00000000");
    }
}

///
/// [合约条件订单/交易更新推送](https://developers.binance.com/docs/zh-CN/derivatives/portfolio-margin/user-data-streams/Event-Conditional-Order-Trade-Update)
///
/// ```json
/// {
///     "e": "CONDITIONAL_ORDER_TRADE_UPDATE", // 时间类型
///     "T": 1669262908216,                    // 交易时间
///     "E": 1669262908218,                    // 事件时间
///     "fs": "UM",                            // 业务线
///     "so": {
///             "s": "BTCUSDT",                // 交易对
///             "c":"TEST",                    // 用户自定义策略Id
///             "si": 176057039,               // 策略Id
///             "S":"SELL",                    // 方向
///             "st": "TRAILING_STOP_MARKET",  // 策略类型
///             "f":"GTC",                     // 生效时间
///             "q":"0.001",                   // 数量
///             "p":"0",                       // 价格
///             "sp":"7103.04",                // TPSL触发价
///             "os":"NEW",                    // 策略订单状态
///             "T":1568879465650,             // 订单
///             "ut": 1669262908216,           // Order update Time
///             "R":false,                     // 仅减仓
///             "wt":"MARK_PRICE",             // TPSL触发价格类型
///             "ps":"LONG",                   // 仓位方向
///             "cp":false,                    // 是否为触发平仓单; 仅在条件订单情况下会推送此字段
///             "AP":"7476.89",                // 追踪止损激活价格, 仅在追踪止损单时会推送此字段
///             "cr":"5.0",                    // 追踪止损回调比例, 仅在追踪止损单时会推送此字段
///             "i":8886774,                   // 订单Id
///             "V":"EXPIRE_TAKER",         // STP mode
///             "gtd":0
///         }
/// }
/// ```
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ConditionalOrderTradeUpdatePayload {
    #[serde(rename = "e")]
    pub event: String, // 事件类型，例如 "CONDITIONAL_ORDER_TRADE_UPDATE"

    #[serde(rename = "T")]
    pub trade_time: u64, // 交易时间（毫秒时间戳）

    #[serde(rename = "E")]
    pub event_time: u64, // 事件接收时间（毫秒时间戳）

    #[serde(rename = "fs")]
    pub business: String, // 业务线标识，例如 "UM" 或 "CM"

    #[serde(rename = "so")]
    pub strategy: Option<ConditionalOrderTradeUpdate>, // 策略/订单详情（可选）
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ConditionalOrderTradeUpdate {
    #[serde(rename = "s")]
    pub symbol: String, // 交易对，全部大写，例如 BTCUSDT

    #[serde(rename = "c")]
    pub client_strategy_id: Option<String>, // 客户自定义策略 ID（可选）

    #[serde(rename = "si")]
    pub strategy_id: u64, // 平台侧策略 ID（可选）

    #[serde(rename = "S")]
    pub side: String, // 买卖方向，BUY 或 SELL（可选）

    #[serde(rename = "st")]
    pub strategy_type: String, // 策略类型，例如 TRAILING_STOP_MARKET（可选）

    #[serde(rename = "f")]
    pub time_in_force: String, // 有效方式（GTC, IOC 等，可选）

    #[serde(rename = "q", with = "string_to_decimal")]
    pub quantity: Decimal, // 订单数量（Decimal，字符串反序列化，可选）

    #[serde(rename = "p", with = "string_to_decimal")]
    pub price: Decimal, // 订单价格（Decimal，可选）

    #[serde(rename = "sp", with = "string_to_decimal")]
    pub stop_price: Decimal, // TPSL 触发价或止损价（可选）

    #[serde(rename = "os")]
    pub order_status: String, // 策略订单状态（可选）

    #[serde(rename = "T")]
    pub order_time: u64, // 订单时间（毫秒时间戳，可选）

    #[serde(rename = "ut")]
    pub order_update_time: u64, // 订单更新时间（毫秒时间戳，可选）

    #[serde(rename = "R")]
    pub reduce_only: Option<bool>, // 是否为仅减仓订单（可选）

    #[serde(rename = "wt")]
    pub trigger_price_type: String, // 触发价格类型（例如 MARK_PRICE，可选）

    #[serde(rename = "ps")]
    pub position_side: String, // 仓位方向（LONG/SHORT/BOTH 等，可选）

    #[serde(rename = "cp")]
    pub is_close_position: bool, // 是否为触发平仓单（可选）

    #[serde(rename = "AP", with = "string_to_decimal")]
    pub activation_price: Decimal, // 激活价格，仅追踪止损单时存在（可选）

    #[serde(rename = "cr", with = "string_to_decimal")]
    pub callback_rate: Decimal, // 回调比例，仅追踪止损单时存在（可选）

    #[serde(rename = "i")]
    pub order_id: u64, // 平台订单 ID（可选）

    #[serde(rename = "V")]
    pub stp_mode: String, // Self-trade prevention 模式（可选）

    #[serde(rename = "gtd")]
    pub gtd: u64, // gtd 字段，含义按交易所文档（可选）
}

///
/// [杠杆账户全仓挂单占用事件](https://developers.binance.com/docs/zh-CN/derivatives/portfolio-margin/user-data-streams/Event-OpenOrderLoss-Update)
///
/// ```json
/// {
///     "e": "openOrderLoss",      //Event Type
///     "E": 1678710578788,        // Event Time
///     "O": [
///         {                    // Update Data
///         "a": "BUSD",
///        "o": "-0.1232313"       // Amount
///         },
///         {
///         "a": "BNB",
///         "o": "-12.1232313"
///         }
///     ]
/// }
/// ```
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OpenOrderLossPayload {
    #[serde(rename = "e")]
    pub event: String, // 事件类型，例如 "openOrderLoss"

    #[serde(rename = "E")]
    pub event_time: u64, // 事件发生时间（毫秒时间戳）

    #[serde(rename = "O")]
    pub updates: Option<Vec<OpenOrderLossItem>>, // 更新项列表（可选）
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OpenOrderLossItem {
    #[serde(rename = "a")]
    pub asset: String, // 资产代码，例如 USDT、BUSD

    #[serde(rename = "o", with = "string_to_decimal")]
    pub amount: Decimal, // 变动金额（Decimal，字符串反序列化）
}

///
/// [杠杆账户更新事件](https://developers.binance.com/docs/zh-CN/derivatives/portfolio-margin/user-data-streams/Event-Margin-Account-Update)
///
/// ```json
/// {
///   "e": "outboundAccountPosition", // 事件类型
///   "E": 1564034571105,             // 事件时间
///   "u": 1564034571073,             // 账户末次更新时间戳
///   "U": 1027053479517,             // 时间更新ID
///   "B": [                          // 余额
///     {
///       "a": "ETH",                 // 资产名称
///       "f": "10000.000000",        // 可用余额
///       "l": "0.000000"             // 冻结余额
///     }
///   ]
/// }
/// ```
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OutboundAccountPositionPayload {
    #[serde(rename = "e")]
    pub event: String, // 事件类型，例如 "outboundAccountPosition"

    #[serde(rename = "E")]
    pub event_time: u64, // 事件时间（毫秒时间戳）

    #[serde(rename = "u")]
    pub last_account_update: u64, // 账户最后更新时间（毫秒时间戳）

    #[serde(rename = "U")]
    pub update_id: u64, // 更新 ID

    #[serde(rename = "B")]
    pub balances: Option<Vec<AccountBalance>>, // 资产余额列表（可选）
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AccountBalance {
    #[serde(rename = "a")]
    pub asset: String, // 资产代码，例如 ETH

    #[serde(rename = "f", with = "string_to_decimal")]
    pub free: Decimal, // 可用余额（Decimal）

    #[serde(rename = "l", with = "string_to_decimal")]
    pub locked: Decimal, // 冻结余额（Decimal）
}

///
/// [杠杆账户负债更新](https://developers.binance.com/docs/zh-CN/derivatives/portfolio-margin/user-data-streams/Event-Liability-Update)
///
/// ```json
///{
///  "e": "liabilityChange",       //Event Type
///   "E": 1573200697110,           //Event Time
///   "a": "BTC",                   //Asset
///   "t": "BORROW",                //Type
///   "T": 1352286576452864727,    //Transaction ID
///   "p": "1.03453430",            //Principal
///   "i": "0",                     //Interest
///   "l": "1.03476851"             //Total Liability
/// }
///
/// ```
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LiabilityChangePayload {
    #[serde(rename = "e")]
    pub event: String, // 事件类型，例如 "liabilityChange"

    #[serde(rename = "E")]
    pub event_time: u64, // 事件时间（毫秒时间戳）

    #[serde(rename = "a")]
    pub asset: String, // 资产代码

    #[serde(rename = "t")]
    pub change_type: String, // 变动类型，例如 BORROW/REPAY（可选）

    #[serde(rename = "T")]
    pub transaction_id: u64, // 交易流水 ID（可选）

    #[serde(rename = "p", with = "string_to_decimal")]
    pub principal: Decimal, // 本金（Decimal，可选）

    #[serde(rename = "i", with = "string_to_decimal")]
    pub interest: Decimal, // 利息（Decimal，可选）

    #[serde(rename = "l", with = "string_to_decimal")]
    pub total_liability: Decimal, // 总负债（Decimal，可选）
}

///
/// [杠杆账户订单事件](https://developers.binance.com/docs/zh-CN/derivatives/portfolio-margin/user-data-streams/Event-Margin-Order-Update)
///
/// ```json
/// {
///   "e": "executionReport",        // 事件类型
///   "E": 1499405658658,            // 事件时间
///   "s": "ETHBTC",                 // 交易对
///   "c": "mUvoqJxFIILMdfAW5iGSOW", // clientOrderId
///   "S": "BUY",                    // 订单方向
///   "o": "LIMIT",                  // 订单类型
///   "f": "GTC",                    // 有效方式
///   "q": "1.00000000",             // 订单原始数量
///   "p": "0.10264410",             // 订单原始价格
///   "P": "0.00000000",             // 止盈止损单触发价格
///   "F": "0.00000000",             // 冰山订单数量; 这仅在冰山订单可见
///   "g": -1,                       // OCO订单 OrderListId
///   "C": "",                       // 原始订单自定义ID(原始订单，指撤单操作的对象。撤单本身被视为另一个订单); 这仅在撤单可见
///   "x": "NEW",                    // 本次事件的具体执行类型
///   "X": "NEW",                    // 订单的当前状态
///   "r": "NONE",                   // 订单被拒绝的原因
///   "i": 4293153,                  // orderId
///   "l": "0.00000000",             // 订单末次成交量
///   "z": "0.00000000",             // 订单累计已成交量
///   "L": "0.00000000",             // 订单末次成交价格
///   "n": "0",                      // 手续费数量
///   "N": null,                     // 手续费资产类别; 这仅在非零的手续费可见
///   "T": 1499405658657,            // 成交时间
///   "t": -1,                       // 成交ID
///  "v": 3,                        // 被阻止撮合交易的ID; 这仅在订单因 STP 触发而过期时可见
///   "I": 8641984,                  // updateId
///   "w": true,                     // 订单是否在订单簿上？
///   "m": false,                    // 该成交是作为挂单成交吗？
///   "O": 1499405658657,            // 订单创建时间
///   "Z": "0.00000000",             // 订单累计已成交金额
///   "Y": "0.00000000",             // 订单末次成交金额
///   "Q": "0.00000000",             // Quote Order Quantity; 这仅在订单中明确标明可见
///   "W": 1499405658657,            // Working Time; 订单被添加到 order book 的时间
///   "V": "NONE"                    // SelfTradePreventionMode
/// }
/// ```
///
///

#[derive(Debug, Deserialize, Serialize, Clone, Message)]
#[rtype(result = "()")]
pub struct ExecutionReportPayload {
    #[serde(rename = "e")]
    pub event: String, // 事件类型，例如 "executionReport"

    #[serde(rename = "E")]
    pub event_time: u64, // 事件时间（毫秒时间戳）

    #[serde(rename = "s")]
    pub symbol: String, // 交易对（可选）

    #[serde(rename = "c")]
    pub client_order_id: Option<String>, // 客户端订单 ID（可选）

    #[serde(rename = "S")]
    pub side: String, // 方向（BUY/SELL，可选）

    #[serde(rename = "o")]
    pub order_type: String, // 订单类型（LIMIT, MARKET 等，可选）

    #[serde(rename = "f")]
    pub time_in_force: String, // 有效方式（GTC 等，可选）

    #[serde(rename = "q", with = "string_to_decimal")]
    pub quantity: Decimal, // 原始数量（Decimal，可选）

    #[serde(rename = "p", with = "string_to_decimal")]
    pub price: Decimal, // 原始价格（Decimal，可选）

    #[serde(rename = "P", with = "string_to_option_decimal")]
    pub stop_price: Option<Decimal>, // 与 SpotOrderPo 保持一致

    #[serde(rename = "F", with = "string_to_option_decimal")]
    pub iceberg_qty: Option<Decimal>, // 可选

    #[serde(rename = "g")]
    pub order_list_id: i64, // OrderListId

    #[serde(rename = "C")]
    pub original_client_order_id: String, // 原始订单的自定义 ID

    #[serde(rename = "x")]
    pub execution_type: String, // 本次事件的执行类型（可选）

    #[serde(rename = "X")]
    pub order_status: String, // 当前订单状态（可选）

    #[serde(rename = "r")]
    pub reject_reason: Option<String>, // 拒单原因（可选）

    #[serde(rename = "i")]
    pub order_id: i64, // 平台订单 ID

    #[serde(rename = "l", with = "string_to_option_decimal")]
    pub last_executed_qty: Option<Decimal>, // 与 SpotOrderPo 保持一致

    #[serde(rename = "z", with = "string_to_option_decimal")]
    pub cumulative_filled_qty: Option<Decimal>, // 与 SpotOrderPo 保持一致

    #[serde(rename = "L", with = "string_to_option_decimal")]
    pub last_executed_price: Option<Decimal>, // 与 SpotOrderPo 保持一致

    #[serde(rename = "n", with = "string_to_option_decimal")]
    pub commission_amount: Option<Decimal>, // 与 SpotOrderPo 保持一致

    #[serde(rename = "N")]
    pub commission_asset: Option<String>, // 手续费资产（可选）

    #[serde(rename = "T")]
    pub trade_time: u64, // 成交时间

    #[serde(rename = "t")]
    pub trade_id: i64, // 成交 ID

    #[serde(rename = "v")]
    pub stp: Option<i64>, // STP 相关字段（可选）

    #[serde(rename = "I")]
    pub update_id: Option<u64>, // updateId

    #[serde(rename = "w")]
    pub is_working: bool, // 是否仍在订单簿上（可选）

    #[serde(rename = "m")]
    pub is_maker: bool, // 本次成交是否为挂单方（可选）

    #[serde(rename = "O")]
    pub order_create_time: u64, // 订单创建时间

    #[serde(rename = "Z", with = "string_to_decimal")]
    pub cumulative_quote_qty: Decimal, // 累计成交金额（保持非可选）

    #[serde(rename = "Y", with = "string_to_option_decimal")]
    pub last_quote_qty: Option<Decimal>, // 与 SpotOrderPo 保持一致

    #[serde(rename = "Q", with = "string_to_option_decimal")]
    pub quote_order_quantity: Option<Decimal>, // 与 SpotOrderPo 保持一致

    #[serde(rename = "W")]
    pub working_time: u64, // 工作时间（可选）订单被添加到 order book 的时间

    #[serde(rename = "V")]
    pub self_trade_prevention_mode: String, // 自交易防护模式（可选）
}

///
/// [合约订单/交易更新推送](https://developers.binance.com/docs/zh-CN/derivatives/portfolio-margin/user-data-streams/Event-Futures-Order-update)
///
/// ```json
/// {
///   "e":"ORDER_TRADE_UPDATE",			// 事件类型
///   "E":1568879465651,				// 事件时间
///   "T":1568879465650,				// 撮合时间
///   "fs": "UM",                   // 事件业务线：'UM'代表U本位合约，'CM'代表币本位合约
///   "o":{
///     "s":"BTCUSDT",					// 交易对
///     "c":"TEST",						// 客户端自定订单ID
///       // 特殊的自定义订单ID:
///       // "autoclose-"开头的字符串: 系统强平订单
///       // "adl_autoclose": ADL自动减仓订单
///       // "settlement_autoclose-": 下架或交割的结算订单
///     "S":"SELL",						// 订单方向
///     "o":"TRAILING_STOP_MARKET",	// 订单类型
///     "f":"GTC",						// 有效方式
///     "q":"0.001",					// 订单原始数量
///     "p":"0",						// 订单原始价格
///     "ap":"0",						// 订单平均价格
///    "sp":"7103.04",				// 忽略
///     "x":"NEW",						// 本次事件的具体执行类型
///     "X":"NEW",						// 订单的当前状态
///     "i":8886774,					// 订单ID
///     "l":"0",						// 订单末次成交量
///     "z":"0",						// 订单累计已成交量
///     "L":"0",						// 订单末次成交价格
///     "N": "USDT",           // 手续费资产类型
///     "n": "0",              // 手续费数量
///     "T":1568879465650,		 // 成交时间
///     "t":0,							   // 成交ID
///     "b":"0",						   // 买单净值
///     "a":"9.91",						 // 卖单净值
///     "m": false,					   // 该成交是作为挂单成交吗？
///     "R":false	,				     // 是否是只减仓单
///     "ps":"LONG"						 // 持仓方向
///     "rp":"0",					     // 该交易实现盈亏
///     "st":"C_TAKE_PROFIT",  // 策略单类型，仅在条件订单触发后会推送此字段
///     "si":12893,\t\t\t\t\t\t\t// 该交易实现盈亏，仅在条件订单触发后会推送此字段
///     "V":"EXPIRE_TAKER",         // STP mode
///     "gtd":0
///   }
/// }
/// ```
///

#[derive(Debug, Deserialize, Serialize, Clone, Message)]
#[rtype(result = "()")]
pub struct OrderTradeUpdatePayload {
    #[serde(rename = "e")]
    pub event: String, // 事件类型，例如 "ORDER_TRADE_UPDATE"

    #[serde(rename = "E")]
    pub event_time: u64, // 事件时间（毫秒时间戳）

    #[serde(rename = "T")]
    pub trade_time: u64, // 撮合时间/成交时间（毫秒时间戳）

    #[serde(rename = "fs")]
    pub business: String, // 业务线（可选）

    #[serde(rename = "o")]
    pub order: OrderTradeInfo, // 订单信息（可选）
}

#[derive(Debug, Deserialize, Serialize, Clone, Message)]
#[rtype(result = "()")]
pub struct OrderTradeInfo {
    #[serde(rename = "s")]
    pub symbol: String, // 交易对（可选）

    #[serde(rename = "c")]
    pub client_order_id: Option<String>, // 客户端订单 ID（可选）

    #[serde(rename = "S")]
    pub side: String, // 方向（可选）

    #[serde(rename = "o")]
    pub order_type: String, // 订单类型（可选）

    #[serde(rename = "f")]
    pub time_in_force: String, // 有效方式（可选）

    #[serde(rename = "q", with = "string_to_decimal")]
    pub quantity: Decimal, // 原始数量（Decimal，可选）

    #[serde(rename = "p", with = "string_to_decimal")]
    pub price: Decimal, // 原始价格（Decimal，可选）

    #[serde(rename = "ap", with = "string_to_option_decimal")]
    pub avg_price: Option<Decimal>, // 平均成交价（Decimal，可选）

    #[serde(rename = "sp", with = "string_to_option_decimal")]
    pub stop_price: Option<Decimal>, // 忽略

    #[serde(rename = "x")]
    pub execution_type: String, // 本次事件执行类型（可选）

    #[serde(rename = "X")]
    pub current_order_status: String, // 当前订单状态（可选）

    #[serde(rename = "i")]
    pub order_id: u64, // 平台订单 ID（可选）

    #[serde(rename = "l", with = "string_to_option_decimal")]
    pub last_filled_qty: Option<Decimal>, // 本次成交数量（可选）

    #[serde(rename = "z", with = "string_to_option_decimal")]
    pub executed_qty: Option<Decimal>, // 累计成交数量（可选）

    #[serde(rename = "L", with = "string_to_decimal")]
    pub last_filled_price: Decimal, // 本次成交价格（可选）

    #[serde(rename = "N")]
    pub fee_asset: String, // 手续费资产（可选）

    #[serde(rename = "n", with = "string_to_decimal")]
    pub fee: Decimal, // 手续费金额（可选）

    #[serde(rename = "T")]
    pub trade_time: u64, // 成交时间（可选）

    #[serde(rename = "t")]
    pub trade_id: i64, // 成交 ID（可选）

    #[serde(rename = "b", with = "string_to_decimal")]
    pub buyer_order_gross: Decimal, // 买单净值（可选）

    #[serde(rename = "a", with = "string_to_decimal")]
    pub seller_order_gross: Decimal, // 卖单净值（可选）

    #[serde(rename = "m")]
    pub is_maker: Option<bool>, // 是否为挂单方（可选）

    #[serde(rename = "R")]
    pub is_reduce_only: bool, // 是否只减仓（可选）

    #[serde(rename = "ps")]
    pub position_side: String, // 仓位方向（可选）

    #[serde(rename = "rp", with = "string_to_decimal")]
    pub realized_pnl: Decimal, // 已实现盈亏（Decimal，可选）

    #[serde(rename = "st")]
    pub strategy_type: Option<String>, // 策略单类型，仅在条件订单触发后会推送此字段

    #[serde(rename = "si")]
    pub order_realized_pnl: Decimal, // 该交易实现盈亏，仅在条件订单触发后会推送此字段

    #[serde(rename = "v")]
    pub stp_mode: String, // STP mode

    #[serde(rename = "gtd")]
    pub gtd: Option<u64>, // gtd 字段（可选）
}

///
/// [合约Balance和Position更新推送](https://developers.binance.com/docs/zh-CN/derivatives/portfolio-margin/user-data-streams/Event-Futures-Balance-and-Position-Update)
///
/// ```json
/// {
///   "e": "ACCOUNT_UPDATE",                // Event Type
///   "fs": "UM",                           // Event business unit. 'UM' for USDS-M futures and 'CM' for COIN-M futures
///   "E": 1564745798939,                   // Event Time
///   "T": 1564745798938 ,                  // Transaction
///   "i":"",                           // Account Alias, ignore for UM
///   "a":                                  // Update Data
///     {
///       "m":"ORDER",                      // Event reason type
///       "B":[                             // Balances
///         {
///           "a":"USDT",                   // Asset
///           "wb":"122624.12345678",       // Wallet Balance
///           "cw":"100.12345678",          // Cross Wallet Balance
///           "bc":"50.12345678"            // Balance Change except PnL and Commission
///         },
///         {
///           "a":"BUSD",
///           "wb":"1.00000000",
///           "cw":"0.00000000",
///           "bc":"-49.12345678"
///         }
///       ],
///      "P":[
///         {
///           "s":"BTCUSDT",            // Symbol
///           "pa":"0",                 // Position Amount
///           "ep":"0.00000",            // Entry Price
///           "cr":"200",               // (Pre-fee) Accumulated Realized
///           "up":"0",                     // Unrealized PnL
///           "ps":"BOTH",                   // Position Side
///           "bep":"0.00000"            // breakeven price}，
///         },
///         {
///             "s":"BTCUSDT",
///             "pa":"20",
///             "ep":"6563.66500",
///             "cr":"0",
///             "up":"2850.21200",
///             "ps":"LONG",
///             "bep":"0.00000"            // breakeven price
///          }
///       ]
///    }
/// }
/// ```
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AccountUpdatePayload {
    #[serde(rename = "e")]
    pub event: String, // 事件类型，例如 "ACCOUNT_UPDATE"

    #[serde(rename = "fs")]
    pub business: String, // 业务线（可选）

    #[serde(rename = "E")]
    pub event_time: u64, // 事件时间（毫秒时间戳）

    #[serde(rename = "T")]
    pub transaction: u64, // 事务/交易时间戳（毫秒）

    #[serde(rename = "i")]
    pub account_alias: Option<String>, // 账户别名（可选）

    #[serde(rename = "a")]
    pub account: Option<AccountUpdateData>, // 账户变更数据（可选）
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AccountUpdateData {
    #[serde(rename = "m")]
    pub reason: String, // 事件原因类型（可选）

    #[serde(rename = "B")]
    pub balances: Option<Vec<AccountBalanceChange>>, // 余额变更列表（可选）

    #[serde(rename = "P")]
    pub positions: Option<Vec<AccountPosition>>, // 持仓列表（可选）
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AccountBalanceChange {
    #[serde(rename = "a")]
    pub asset: String, // 资产代码

    #[serde(rename = "wb", with = "string_to_decimal")]
    pub wallet_balance: Decimal, // 钱包余额（可选，Decimal）

    #[serde(rename = "cw", with = "string_to_decimal")]
    pub cross_wallet_balance: Decimal, // 全仓钱包余额（可选，Decimal）

    #[serde(rename = "bc", with = "string_to_decimal")]
    pub balance_change: Decimal, // 余额变动（可选，Decimal）
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AccountPosition {
    #[serde(rename = "s")]
    pub symbol: String, // 交易对

    #[serde(rename = "pa", with = "string_to_decimal")]
    pub position_amount: Decimal, // 持仓数量（可选）

    #[serde(rename = "ep", with = "string_to_decimal")]
    pub entry_price: Decimal, // 开仓均价（可选）

    #[serde(rename = "cr", with = "string_to_decimal")]
    pub cumulative_realized: Decimal, // 累计已实现盈亏（可选）

    #[serde(rename = "up", with = "string_to_decimal")]
    pub unrealized_pnl: Decimal, // 未实现盈亏（可选）

    #[serde(rename = "ps")]
    pub position_side: String, // 仓位方向（可选）

    #[serde(rename = "bep", with = "string_to_decimal")]
    pub break_even_price: Decimal, // 保本价格（可选）
}

///
/// [合约杠杆倍数等账户配置更新推送](https://developers.binance.com/docs/zh-CN/derivatives/portfolio-margin/user-data-streams/Event-Futures-Account-Configuration-Update)
///
/// ```json
///{
///     "e":"ACCOUNT_CONFIG_UPDATE",       // 事件类型
///     "fs": "UM",                       // 事件业务线
///     "E":1611646737479,                 // 事件时间
///     "T":1611646737476,                 // 撮合时间
///     "ac":{
///     "s":"BTCUSD_PERP",                     // 交易对
///     "l":25                             // 杠杆倍数
///     }
/// }
/// ```
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AccountConfigUpdatePayload {
    #[serde(rename = "e")]
    pub event: String, // 事件类型，例如 "ACCOUNT_CONFIG_UPDATE"

    #[serde(rename = "fs")]
    pub business: String, // 业务线（可选）

    #[serde(rename = "E")]
    pub event_time: u64, // 事件时间（毫秒时间戳）

    #[serde(rename = "T")]
    pub trade_time: u64, // 撮合时间/交易时间（毫秒时间戳）

    #[serde(rename = "ac")]
    pub ac: Option<AccountConfigItem>, // 账户配置项（可选）
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AccountConfigItem {
    #[serde(rename = "s")]
    pub symbol: String, // 交易对（可选）

    #[serde(rename = "l")]
    pub leverage: i32, // 杠杆倍数（可选）
}

/// [账户风险状态变动](https://developers.binance.com/docs/zh-CN/derivatives/portfolio-margin/user-data-streams/Event-riskLevelChange)
///
/// ```json
/// {
///     "e":"riskLevelChange",  // 事件类型
///     "E":1587727187525,      // 事件时间
///     "u":"1.99999999",       // uniMMR
///     "s":"MARGIN_CALL",      //MARGIN_CALL, REDUCE_ONLY, FORCE_LIQUIDATION
///     "eq":"30.23416728",     // 账号美元保证金
///     "ae":"30.23416728",     // actual equity without collateral rate in USD value
///     "m":"15.11708371"       // 美元计价维持保证金
/// }
/// ```
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RiskLevelChangePayload {
    #[serde(rename = "e")]
    pub event: String, // 事件类型，例如 "riskLevelChange"

    #[serde(rename = "E")]
    pub event_time: u64, // 事件时间（毫秒时间戳）

    #[serde(rename = "u", with = "string_to_decimal")]
    pub uni_mmr: Decimal, // uniMMR 值（可选，Decimal）

    #[serde(rename = "s")]
    pub status: String, // 风险状态（MARGIN_CALL、REDUCE_ONLY 等，可选）

    #[serde(rename = "eq", with = "string_to_decimal")]
    pub equity: Decimal, // 账户美元计价净值（可选，Decimal）

    #[serde(rename = "ae", with = "string_to_decimal")]
    pub actual_equity: Decimal, // 不含抵押率的实际权益（可选，Decimal）

    #[serde(rename = "m", with = "string_to_decimal")]
    pub maintenance_margin: Decimal, // 维持保证金（可选，Decimal）
}

///
/// [杠杆账户余额更新事件](https://developers.binance.com/docs/zh-CN/derivatives/portfolio-margin/user-data-streams/Event-Margin-Balance-Update)
///
/// ```json
/// {
///   "e": "balanceUpdate",         //时间类型
///   "E": 1573200697110,           //事件时间
///   "a": "BTC",                   //资产
///   "d": "100.00000000",          //变动数量
///   "U": 1027053479517            //事件更新ID
///   "T": 1573200697068            //Time
/// }
/// ```
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BalanceUpdatePayload {
    #[serde(rename = "e")]
    pub event: String, // 事件类型，例如 "balanceUpdate"

    #[serde(rename = "E")]
    pub event_time: u64, // 事件时间（毫秒时间戳）

    #[serde(rename = "a")]
    pub asset: String, // 资产代码

    #[serde(rename = "d", with = "string_to_decimal")]
    pub delta: Decimal, // 变动数量（Decimal）

    #[serde(rename = "U")]
    pub update_id: u64, // 事件更新 ID（可选）

    #[serde(rename = "T")]
    pub time: u64, // 时间戳（可选）
}
