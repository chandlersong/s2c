use crate::models::Decimal;
use crate::tools::{string_to_decimal, string_to_option_decimal};
use actix::{Message as ActixMessage, Message};
use li::websocket::models::WebSocketMessage;
use serde::{Deserialize, Serialize};

/// 现货账户流枚举，兼容余额、订单事件以及订阅响应。
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum BinanceSpotAccountWebSocketResponse {
    /// 余额更新事件 outboundAccountPosition
    OutboundAccountPosition(AccountWebSocketPayLoad<OutboundAccountPositionPayload>),
    /// 单个资产余额变动事件 balanceUpdate
    BalanceUpdate(AccountWebSocketPayLoad<BalanceUpdatePayload>),
    /// 订单执行报告 executionReport
    ExecutionReport(AccountWebSocketPayLoad<ExecutionReportPayload>),
    /// 订阅/管理指令返回（无 e 字段） - 必须放最后，因为字段都是 Optional
    SubscribeResponse(SubscribeResponsePayload),
}

impl WebSocketMessage for BinanceSpotAccountWebSocketResponse {
    fn from_text(text: &str) -> Result<Self, li::errors::LiError> {
        serde_json::from_str(text).map_err(|e| li::errors::LiError::from(e))
    }
}

impl ActixMessage for BinanceSpotAccountWebSocketResponse {
    type Result = ();
}

impl BinanceSpotAccountWebSocketResponse {
    pub fn from_text(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AccountWebSocketPayLoad<T> {
    #[serde(rename = "subscriptionId")]
    pub subscription_id: u64,

    #[serde(rename = "event")]
    pub event: T,
}

/// 账户余额更新事件载荷，对应 outboundAccountPosition。
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OutboundAccountPositionPayload {
    #[serde(rename = "e")]
    pub event: String,
    #[serde(rename = "E")]
    pub event_time: u64,
    #[serde(rename = "u")]
    pub last_account_update: u64,
    #[serde(rename = "B")]
    pub balances: Vec<BalanceItem>,
}

/// 单个资产余额信息。
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BalanceItem {
    /// 资产名称
    #[serde(rename = "a")]
    pub asset: String,
    /// 可用余额
    #[serde(rename = "f")]
    #[serde(with = "string_to_decimal")]
    pub free: Decimal,
    /// 锁定余额(订单、合约等冻结)
    #[serde(rename = "l")]
    #[serde(with = "string_to_decimal")]
    pub locked: Decimal,
}

/// 单个资产余额变动事件载荷，对应 balanceUpdate。
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BalanceUpdatePayload {
    /// 事件类型: balanceUpdate
    #[serde(rename = "e")]
    pub event: String,
    /// 事件时间戳(毫秒)
    #[serde(rename = "E")]
    pub event_time: u64,
    /// 资产名称
    #[serde(rename = "a")]
    pub asset: String,
    /// 余额变动量
    #[serde(rename = "d")]
    #[serde(with = "string_to_decimal")]
    pub balance_delta: Decimal,
    /// 清算时间(毫秒)
    #[serde(rename = "T")]
    pub clear_time: u64,
}
/// 订单执行报告载荷，对应 executionReport。
/// [现货术语表](https://developers.binance.com/docs/zh-CN/binance-spot-api-docs/faqs/spot_glossary)
#[derive(Debug, Deserialize, Serialize, Clone, Message)]
#[rtype(result = "()")]
pub struct ExecutionReportPayload {
    // === 按示例顺序排列（必填或常见字段在前） ===
    /// 事件类型: executionReport
    #[serde(rename = "e")]
    pub event: String,
    /// 事件时间戳(毫秒)
    #[serde(rename = "E")]
    pub event_time: u64,
    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,
    /// 客户端订单ID
    #[serde(rename = "c")]
    pub client_order_id: Option<String>,
    /// 订单方向: BUY/SELL
    #[serde(rename = "S")]
    pub side: String,
    /// 订单类型: LIMIT/MARKET/等
    #[serde(rename = "o")]
    pub order_type: String,
    /// 有效期: GTC/IOC/FOK/等
    #[serde(rename = "f")]
    pub time_in_force: String,
    /// 订单数量
    #[serde(rename = "q")]
    #[serde(with = "string_to_decimal")]
    pub order_qty: Decimal,
    /// 订单价格
    #[serde(rename = "p")]
    #[serde(with = "string_to_decimal")]
    pub order_price: Decimal,
    /// 止盈止损单触发价格（仅止盈/止损单出现）
    #[serde(rename = "P", with = "string_to_option_decimal")]
    pub stop_price: Option<Decimal>,
    /// 冰山订单数量（仅冰山单出现）
    #[serde(rename = "F", with = "string_to_option_decimal")]
    pub iceberg_qty: Option<Decimal>,
    /// OCO订单 OrderListId（仅OCO出现）
    #[serde(rename = "g")]
    pub order_list_id: i64,
    /// 原始订单自定义ID（仅修改/撤单时出现）
    #[serde(rename = "C")]
    pub original_client_order_id: String,
    /// 本次事件的具体执行类型
    #[serde(rename = "x")]
    pub execution_type: String,
    /// 订单的当前状态
    #[serde(rename = "X")]
    pub order_status: String,
    /// 订单被拒绝的原因
    #[serde(rename = "r")]
    pub reject_reason: Option<String>,
    /// orderId
    #[serde(rename = "i")]
    pub order_id: i64,
    /// 订单末次成交量（仅TRADE出现）
    #[serde(rename = "l", with = "string_to_option_decimal")]
    pub last_executed_qty: Option<Decimal>,
    /// 订单累计已成交量
    #[serde(rename = "z", with = "string_to_option_decimal")]
    pub cumulative_filled_qty: Option<Decimal>,
    /// 订单末次成交价格（仅TRADE出现）
    #[serde(rename = "L", with = "string_to_option_decimal")]
    pub last_executed_price: Option<Decimal>,
    /// 手续费数量（仅TRADE出现）
    #[serde(rename = "n", with = "string_to_option_decimal")]
    pub commission_amount: Option<Decimal>,
    /// 手续费资产类别（仅TRADE出现）
    #[serde(rename = "N")]
    pub commission_asset: Option<String>,
    /// 成交时间
    #[serde(rename = "T")]
    pub trade_time: u64,
    /// Trade ID（仅TRADE可能出现）
    #[serde(rename = "t")]
    pub trade_id: i64,
    /// 被阻止的交易Id（仅STP阻止出现）
    #[serde(rename = "v")]
    #[serde(default)]
    pub stp: Option<i64>,
    /// Execution ID / 订单创建时间（以现实现字段为准）
    #[serde(rename = "I")]
    pub order_creation_time: u64,
    /// 订单是否在订单簿上
    #[serde(rename = "w")]
    pub is_working: bool,
    /// 该成交是否作为挂单成交（仅TRADE出现）
    #[serde(rename = "m")]
    pub is_maker: bool,
    /// 是否最优匹配（仅TRADE出现）
    #[serde(rename = "M")]
    pub is_best_match: bool,
    /// 订单创建时间
    #[serde(rename = "O")]
    pub order_create_time: u64,
    /// 订单累计已成交金额
    #[serde(rename = "Z", with = "string_to_decimal")]
    pub cumulative_quote_qty: Decimal,
    /// 订单末次成交金额（仅TRADE出现）
    #[serde(rename = "Y", with = "string_to_option_decimal")]
    pub last_quote_qty: Option<Decimal>,
    /// Quote Order Quantity（仅特定订单出现）
    #[serde(rename = "Q", with = "string_to_option_decimal")]
    pub quote_order_quantity: Option<Decimal>,
    /// Working Time；订单被添加到 order book 的时间
    #[serde(rename = "W")]
    pub working_time: u64,
    /// SelfTradePreventionMode
    #[serde(rename = "V")]
    pub self_trade_prevention_mode: String,
    // === 其它可选扩展字段（示例未列出） ===
    /// 追踪止损增量 - 仅在TRAILING_STOP_MARKET订单时出现
    #[serde(rename = "d")]
    #[serde(default, with = "string_to_option_decimal")]
    pub trailing_delta: Option<Decimal>,
    /// 追踪时间戳(毫秒) - 仅在TRAILING_STOP_MARKET订单时出现
    #[serde(rename = "D")]
    pub trailing_time: Option<u64>,
    /// 策略ID - 仅在请求中添加了strategyId参数时出现
    #[serde(rename = "j")]
    #[serde(default)]
    pub strategy_id: Option<u64>,
    /// 策略类型 - 仅在请求中添加了strategyType参数时出现
    #[serde(rename = "J")]
    #[serde(default)]
    pub strategy_type: Option<u64>,

    ///只有在因为 STP 导致订单失效时可见。
    #[serde(rename = "A")]
    #[serde(default, with = "string_to_option_decimal")]
    pub prevented_quantity: Option<Decimal>,
    #[serde(rename = "B")]
    #[serde(default, with = "string_to_option_decimal")]
    pub last_prevented_quantity: Option<Decimal>,
    #[serde(rename = "u")]
    #[serde(default)]
    pub trade_group_id: Option<u64>,
    #[serde(rename = "U")]
    #[serde(default)]
    pub counter_order_id: Option<bool>,
    #[serde(rename = "Cs")]
    #[serde(default)]
    pub counter_symbol: Option<String>,
    #[serde(rename = "pl")]
    #[serde(default, with = "string_to_option_decimal")]
    pub prevented_execution_quantity: Option<Decimal>,
    #[serde(rename = "pL")]
    #[serde(default, with = "string_to_option_decimal")]
    pub prevented_execution_price: Option<Decimal>,
    #[serde(rename = "pY")]
    #[serde(default, with = "string_to_option_decimal")]
    pub prevented_execution_quote_qty: Option<Decimal>,
    /// 只有在订单有分配时可见
    #[serde(rename = "b")]
    #[serde(default)]
    pub match_type: Option<String>,
    #[serde(rename = "a")]
    #[serde(default)]
    pub allocation_id: Option<u64>,
    /// 只有在订单可能有分配时可见
    #[serde(rename = "k")]
    #[serde(default)]
    //只有在订单使用 SOR 时可见
    pub working_floor: Option<String>,
    #[serde(rename = "uS")]
    #[serde(default)]
    pub used_sor: Option<bool>,
    //仅出现在挂钩订单中
    #[serde(rename = "gP")]
    #[serde(default)]
    pub pegged_price_type: Option<String>,
    #[serde(rename = "gOT")]
    #[serde(default)]
    pub pegged_offset_type: Option<String>,
    #[serde(rename = "gOV")]
    #[serde(default)]
    pub pegged_offset_value: Option<u64>,
    #[serde(rename = "gp")]
    #[serde(default, with = "string_to_option_decimal")]
    pub pegged_price: Option<Decimal>,
}

/// 订阅/管理接口返回值，无事件字段。
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SubscribeResponsePayload {
    /// 请求ID
    #[serde(default)]
    pub id: Option<u64>,
    /// 响应状态码
    #[serde(default)]
    pub status: Option<u16>,
    /// 响应结果
    #[serde(default)]
    pub result: Option<Subscription>,
}

/// 监听密钥结果
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Subscription {
    /// 订阅ID
    #[serde(rename = "subscriptionId")]
    pub subscription_id: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_outbound_account_position() {
        let json = r#"{
            "subscriptionId": 123,
            "event": {
                "e":"outboundAccountPosition",
                "E":1690000000000,
                "u":1690000000000,
                "B":[
                    {
                        "a":"BTC",
                        "f":"1.5",
                        "l":"0.5"
                    },
                    {
                        "a":"USDT",
                        "f":"50000.0",
                        "l":"0.0"
                    }
                ]
            }
        }"#;

        let result = BinanceSpotAccountWebSocketResponse::from_text(json).expect("should parse outbound");
        match result {
            BinanceSpotAccountWebSocketResponse::OutboundAccountPosition(p) => {
                assert_eq!(p.subscription_id, 123);
                assert_eq!(p.event.event, "outboundAccountPosition");
                assert_eq!(p.event.event_time, 1690000000000);
                assert_eq!(p.event.last_account_update, 1690000000000);
                assert_eq!(p.event.balances.len(), 2);
                assert_eq!(p.event.balances[0].asset, "BTC");
                assert_eq!(p.event.balances[1].asset, "USDT");
            }
            BinanceSpotAccountWebSocketResponse::BalanceUpdate(_) => {
                panic!("Matched BalanceUpdate instead of OutboundAccountPosition");
            }
            BinanceSpotAccountWebSocketResponse::SubscribeResponse(_) => {
                panic!("Matched SubscribeResponse instead of OutboundAccountPosition");
            }
            BinanceSpotAccountWebSocketResponse::ExecutionReport(_) => {
                panic!("Matched ExecutionReport instead of OutboundAccountPosition");
            }
        }
    }

    #[test]
    fn parse_execution_report() {
        let json = r#"{
            "subscriptionId": 456,
            "event": {
                "e":"executionReport",
                "E":1690000002000,
                "s":"BTCUSDT",
                "c":"web_abc123",
                "S":"BUY",
                "o":"LIMIT",
                "f":"GTC",
                "q":"0.01",
                "p":"30000.5",
                "P":"0",
                "F":"0",
                "g":-1,
                "C":"",
                "x":"TRADE",
                "X":"FILLED",
                "r":"NONE",
                "i":123456789,
                "l":"0.01",
                "z":"0.01",
                "L":"30000.5",
                "n":"0",
                "N":"",
                "T":1690000002001,
                "t":5678,
                "v":null,
                "I":1690000001999,
                "w":true,
                "m":false,
                "M":true,
                "O":1690000001999,
                "Z":"300.005",
                "Y":"300.005",
                "Q":"0.0",
                "W":1690000001999,
                "V":"NONE",
                "d":null,
                "D":null,
                "j":0,
                "J":0,
                "A":"0.0",
                "B":"0.0",
                "u":0,
                "U":false,
                "Cs":"",
                "pl":"0.0",
                "pL":"0.0",
                "pY":"0.0",
                "b":"",
                "a":0,
                "k":"",
                "uS":false,
                "gP":"",
                "gOT":"",
                "gOV":0,
                "gp":null
            }
        }"#;

        let result = BinanceSpotAccountWebSocketResponse::from_text(json).expect("should parse executionReport");
        match result {
            BinanceSpotAccountWebSocketResponse::ExecutionReport(p) => {
                assert_eq!(p.subscription_id, 456);
                assert_eq!(p.event.event, "executionReport");
                assert_eq!(p.event.symbol, "BTCUSDT");
                assert_eq!(p.event.order_status, "FILLED");
                assert_eq!(p.event.order_id, 123456789);
                assert_eq!(p.event.execution_type, "TRADE");
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn parse_subscribe_response() {
        let json = r#"{"id":1,"status":200,"result":{"subscriptionId":12345}}"#;
        let result = BinanceSpotAccountWebSocketResponse::from_text(json).expect("should parse subscribe response");
        match result {
            BinanceSpotAccountWebSocketResponse::SubscribeResponse(p) => {
                assert_eq!(p.id, Some(1));
                assert_eq!(p.status, Some(200));
                assert_eq!(p.result.unwrap().subscription_id, 12345);
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn parse_real_execution_report_new() {
        let json = r#"{
            "subscriptionId": 0,
            "event": {
                "e": "executionReport",
                "E": 1768889505076,
                "s": "BFUSDUSDT",
                "c": "ios",
                "S": "SELL",
                "o": "MARKET",
                "f": "GTC",
                "q": "20.00000000",
                "p": "0.00000000",
                "P": "0.00000000",
                "F": "0.00000000",
                "g": -1,
                "C": "",
                "x": "NEW",
                "X": "NEW",
                "r": "NONE",
                "i": 1328992,
                "l": "0.00000000",
                "z": "0.00000000",
                "L": "0.00000000",
                "n": "0",
                "N": null,
                "T": 1768889505076,
                "t": -1,
                "I": 3575988,
                "w": true,
                "m": false,
                "M": false,
                "O": 1768889505076,
                "Z": "0.00000000",
                "Y": "0.00000000",
                "Q": "0.00000000",
                "W": 1768889505076,
                "V": "EXPIRE_MAKER"
            }
        }"#;

        let result = BinanceSpotAccountWebSocketResponse::from_text(json).expect("should parse real execution report");
        match result {
            BinanceSpotAccountWebSocketResponse::ExecutionReport(p) => {
                assert_eq!(p.event.event, "executionReport");
                assert_eq!(p.event.symbol, "BFUSDUSDT");
                assert_eq!(p.event.order_status, "NEW");
                assert_eq!(p.event.execution_type, "NEW");
                assert_eq!(p.event.commission_asset, None);
                assert_eq!(p.event.stp, None);
                assert_eq!(p.event.strategy_id, None);
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn parse_real_execution_report_filled() {
        let json = r#"{
            "subscriptionId": 0,
            "event": {
                "e": "executionReport",
                "E": 1768889505076,
                "s": "BFUSDUSDT",
                "c": "ios",
                "S": "SELL",
                "o": "MARKET",
                "f": "GTC",
                "q": "20.00000000",
                "p": "0.00000000",
                "P": "0.00000000",
                "F": "0.00000000",
                "g": -1,
                "C": "",
                "x": "TRADE",
                "X": "FILLED",
                "r": "NONE",
                "i": 1328992,
                "l": "20.00000000",
                "z": "20.00000000",
                "L": "0.99960000",
                "n": "0.01999200",
                "N": "USDT",
                "T": 1768889505076,
                "t": 918961,
                "I": 3575989,
                "w": false,
                "m": false,
                "M": true,
                "O": 1768889505076,
                "Z": "19.99200000",
                "Y": "19.99200000",
                "Q": "0.00000000",
                "W": 1768889505076,
                "V": "EXPIRE_MAKER"
            }
        }"#;

        let result = BinanceSpotAccountWebSocketResponse::from_text(json).expect("should parse real filled execution report");
        match result {
            BinanceSpotAccountWebSocketResponse::ExecutionReport(p) => {
                assert_eq!(p.event.event, "executionReport");
                assert_eq!(p.event.symbol, "BFUSDUSDT");
                assert_eq!(p.event.order_status, "FILLED");
                assert_eq!(p.event.execution_type, "TRADE");
                assert_eq!(p.event.commission_asset, Some("USDT".to_string()));
                assert_eq!(p.event.trade_id, 918961);
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn parse_balance_update() {
        let json = r#"{
            "subscriptionId": 0,
            "event": {
                "e": "balanceUpdate",
                "E": 1768889461858,
                "a": "BFUSD",
                "d": "20.00000000",
                "T": 1768889461858
            }
        }"#;

        let result = BinanceSpotAccountWebSocketResponse::from_text(json).expect("should parse balance update");
        match result {
            BinanceSpotAccountWebSocketResponse::BalanceUpdate(p) => {
                assert_eq!(p.subscription_id, 0);
                assert_eq!(p.event.event, "balanceUpdate");
                assert_eq!(p.event.asset, "BFUSD");
                assert_eq!(p.event.event_time, 1768889461858);
                assert_eq!(p.event.clear_time, 1768889461858);
            }
            _ => panic!("unexpected variant - got {:?}", result),
        }
    }
}
