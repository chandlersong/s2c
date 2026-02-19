//! [swap account swap对象](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/user-data-streams/Event-Order-Update)

use crate::models::Decimal;
use crate::tools::string_to_decimal;
use crate::tools::string_to_option_decimal;
use actix::Message as ActixMessage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
/// 期货（Swap / USDS-M）用户数据流顶层反序列化入口，兼容多种事件形态。
pub enum BinanceSwapAccountStreamResponse {
    /// [Balance 和 Position 更新推送]
    AccountUpdate(AccountUpdatePayload),
    /// 账户余额/持仓更新（ACCOUNT_UPDATE）
    MarginCall(MarginCallPayload),
    /// 监听键失效或其他简单事件
    OrderTradeUpdate(OrderTradeUpdatePayload),
    TradeLite(TradeLitePayload),
    AccountConfigUpdate(AccountConfigUpdatePayLoad),
    StrategyUpdatePay(StrategyUpdatePayLoad),
    GridUpdate(GridUpdatePayLoad),
    ConditionalOrderTrigger(ConditionalOrderTriggerRejectPayLoad),
    AlgoUpdate(AlgoUpdatePayload),
    /// 兜底：未匹配到上述任何已知类型时，保留原始 JSON，便于调试或日志打印
    UnKnow(Value),
}

impl ActixMessage for BinanceSwapAccountStreamResponse {
    type Result = ();
}

impl BinanceSwapAccountStreamResponse {
    pub fn from_text(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}

///
/// [Balance 和 Position 更新推送](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/user-data-streams/Event-Balance-and-Position-Update)
///
/// ```json
/// {
///   "e": "ACCOUNT_UPDATE",				// 事件类型
///   "E": 1564745798939,            		// 事件时间
///   "T": 1564745798938 ,           		// 撮合时间
///   "a":                          		// 账户更新事件
///     {
///       "m":"ORDER",						// 事件推出原因
///       "B":[                     		// 余额信息
///        {
///           "a":"USDT",           		// 资产名称
///           "wb":"122624.12345678",    	// 钱包余额
///           "cw":"100.12345678",			// 除去逐仓仓位保证金的钱包余额
///           "bc":"50.12345678"			// 除去盈亏与交易手续费以外的钱包余额改变量
///         },
///         {
///           "a":"BUSD",
///           "wb":"1.00000000",
///           "cw":"0.00000000",
///           "bc":"-49.12345678"
///         }
///       ],
///       "P":[
///        {
///           "s":"BTCUSDT",          	// 交易对
///           "pa":"0",               	// 仓位
///           "ep":"0.00000",            // 入仓价格
///           "bep":"0",                // 盈亏平衡价
///           "cr":"200",             	// (费前)累计实现损益
///           "up":"0",						// 持仓未实现盈亏
///           "mt":"isolated",				// 保证金模式
///           "iw":"0.00000000",			// 若为逐仓，仓位保证金
///           "ps":"BOTH"					// 持仓方向
///        }，
///        {
///         	"s":"BTCUSDT",
///        	"pa":"20",
///         	"ep":"6563.66500",
///         	"bep":"6563.6",
///         	"cr":"0",
///         	"up":"2850.21200",
///         	"mt":"isolated",
///         	"iw":"13200.70726908",
///         	"ps":"LONG"
///       	 },
///        {
///         	"s":"BTCUSDT",
///         	"pa":"-10",
///         	"ep":"6563.86000",
///         	"bep":"6563.6",
///         	"cr":"-45.04000000",
///         	"up":"-1423.15600",
///         	"mt":"isolated",
///         	"iw":"6570.42511771",
///         	"ps":"SHORT"
///        }
///       ]
///     }
/// }
/// ```
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AccountUpdatePayload {
    /// 事件推出原因，例如 "ORDER"
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 撮合时间
    #[serde(rename = "T")]
    pub trade_time: u64,
}
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AccountEvent {
    /// 余额信息数组
    #[serde(rename = "m")]
    pub reason: String,

    /// 余额信息数组
    #[serde(rename = "B")]
    pub balances: Option<Vec<BalanceInfo>>,

    /// 持仓信息数组
    #[serde(rename = "P")]
    pub positions: Option<Vec<PositionInfo>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BalanceInfo {
    /// 资产名称，例如 "USDT"
    #[serde(rename = "a")]
    pub asset: String,

    /// 钱包余额（字符串形式的数值）
    #[serde(rename = "wb", with = "string_to_decimal")]
    pub wallet_balance: Decimal,

    /// 除去逐仓仓位保证金的钱包余额（字符串形式的数值）
    #[serde(rename = "cw", with = "string_to_decimal")]
    pub cross_wallet_balance: Decimal,

    /// 除去盈亏与交易手续费以外的钱包余额改变量（字符串形式的数值）
    #[serde(rename = "bc", with = "string_to_decimal")]
    pub balance_change: Decimal,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PositionInfo {
    /// 交易对，例如 "BTCUSDT"
    #[serde(rename = "s")]
    pub symbol: String,

    /// 仓位（字符串形式的数值）
    #[serde(rename = "pa", with = "string_to_decimal")]
    pub position_amount: Decimal,

    /// 入仓价格（字符串形式的数值）
    #[serde(rename = "ep", with = "string_to_decimal")]
    pub entry_price: Decimal,

    /// 盈亏平衡价（字符串形式的数值）
    #[serde(rename = "bep", with = "string_to_decimal")]
    pub break_even_price: Decimal,

    /// (费前)累计实现损益（字符串形式的数值）
    #[serde(rename = "cr", with = "string_to_decimal")]
    pub cumulative_realized: Decimal,

    /// 持仓未实现盈亏（字符串形式的数值）
    #[serde(rename = "up", with = "string_to_decimal")]
    pub unrealized_pnl: Decimal,

    /// 保证金模式，例如 "isolated" 或 "CROSSED"
    #[serde(rename = "mt")]
    pub margin_type: Option<String>,

    /// 若为逐仓，仓位保证金（字符串形式的数值）
    #[serde(rename = "iw", with = "string_to_decimal")]
    pub isolated_wallet: Decimal,

    /// 持仓方向，例如 "BOTH","LONG","SHORT"
    #[serde(rename = "ps")]
    pub position_side: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
///
/// 追加保证金（Margin Call）事件
/// [追加保证金（Margin Call）事件](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/user-data-streams/Event-Margin-Call)
/// ```json
/// {
///     "e":"MARGIN_CALL",    	// 事件类型
///     "E":1587727187525,		// 事件时间
///     "cw":"3.16812045",		// 除去逐仓仓位保证金的钱包余额, 仅在全仓 margin call 情况下推送此字段
///     "p":[					// 涉及持仓
///       {
///         "s":"ETHUSDT",		// symbol
///         "ps":"LONG",		// 持仓方向
///         "pa":"1.327",		// 仓位
///         "mt":"CROSSED",		// 保证金模式
///         "iw":"0",			// 若为逐仓，仓位保证金
///         "mp":"187.17127",	// 标记价格
///         "up":"-1.166074",	// 未实现盈亏
///         "mm":"1.614445"		// 持仓需要的维持保证金
///       }
///     ]
/// }
/// ```
///
pub struct MarginCallPayload {
    /// 事件时间（毫秒）
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间（毫秒）
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 除去逐仓仓位保证金的钱包余额，仅在全仓 margin call 情况下推送此字段（字符串形式的数值）
    #[serde(rename = "cw", with = "string_to_decimal")]
    pub cross_wallet_balance: Decimal,

    /// 涉及的持仓数组
    #[serde(rename = "p")]
    pub positions: Option<Vec<MarginPosition>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MarginPosition {
    /// 交易对，例如 "ETHUSDT"
    #[serde(rename = "s")]
    pub symbol: String,

    /// 持仓方向，例如 "LONG"/"SHORT"
    #[serde(rename = "ps")]
    pub position_side: String,

    /// 仓位（字符串形式的数值）
    #[serde(rename = "pa", with = "string_to_decimal")]
    pub position_amount: Decimal,

    /// 保证金模式，例如 "CROSSED"（字符串）
    #[serde(rename = "mt")]
    pub margin_type: String,

    /// 若为逐仓，仓位保证金（字符串形式的数值）
    #[serde(rename = "iw", with = "string_to_decimal")]
    pub isolated_wallet: Decimal,

    /// 标记价格（字符串形式的数值）
    #[serde(rename = "mp", with = "string_to_decimal")]
    pub mark_price: Decimal,

    /// 未实现盈亏（字符串形式的数值）
    #[serde(rename = "up", with = "string_to_decimal")]
    pub unrealized_pnl: Decimal,

    /// 持仓需要的维持保证金（字符串形式的数值）
    #[serde(rename = "mm", with = "string_to_decimal")]
    pub maintenance_margin: Decimal,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
/// 订单交易更新推送
/// [订单交易更新推送](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/user-data-streams/Event-Order-Update)
///
/// ```json
///{
///  "e":"ORDER_TRADE_UPDATE",			// 事件类型
///  "E":1568879465651,				    // 事件时间
///  "T":1568879465650,				    // 撮合时间
///  "o":{
///    "s":"BTCUSDT",					    // 交易对
///    "c":"TEST",						      // 客户端自定订单ID
///      // 特殊的自定义订单ID:
///      // "autoclose-"开头的字符串: 系统强平订单
///      // "adl_autoclose": ADL自动减仓订单
///      // "settlement_autoclose-": 下架或交割的结算订单
///    "S":"SELL",						      // 订单方向
///    "o":"TRAILING_STOP_MARKET",	// 订单类型
///    "f":"GTC",						      // 有效方式
///    "q":"0.001",					      // 订单原始数量
///    "p":"0",						        // 订单原始价格
///    "ap":"0",						        // 订单平均价格
///    "sp":"7103.04",			        // 条件订单触发价格，对追踪止损单无效
///    "x":"NEW",						      // 本次事件的具体执行类型
///    "X":"NEW",						      // 订单的当前状态
///    "i":8886774,					      // 订单ID
///    "l":"0",						        // 订单末次成交量
///    "z":"0",						        // 订单累计已成交量
///    "L":"0",						        // 订单末次成交价格
///    "N": "USDT",                // 手续费资产类型
///    "n": "0",                   // 手续费数量
///    "T":1568879465650,				  // 成交时间
///    "t":0,							        // 成交ID
///    "b":"0",						        // 买单净值
///    "a":"9.91",						      // 卖单净值
///    "m": false,					        // 该成交是作为挂单成交吗？
///    "R":false	,				          // 是否是只减仓单
///    "wt": "CONTRACT_PRICE",	    // 触发价类型
///    "ot": "TRAILING_STOP_MARKET",	// 原始订单类型
///    "ps":"LONG"						      // 持仓方向
///    "cp":false,						      // 是否为触发平仓单; 仅在条件订单情况下会推送此字段
///    "AP":"7476.89",					    // 追踪止损激活价格, 仅在追踪止损单时会推送此字段
///    "cr":"5.0",						      // 追踪止损回调比例, 仅在追踪止损单时会推送此字段
///    "pP": false,                // 是否开启条件单触发保护
///    "si": 0,                    // 忽略
///    "ss": 0,                    // 忽略
///    "rp":"0",					          // 该交易实现盈亏
///    "V":"EXPIRE_TAKER",         // 自成交防止模式
///    "pm":"OPPONENT",            // 价格匹配模式
///    "gtd":0,                    // TIF为GTD的订单自动取消时间
///    "er":"0"                    // 过期原因
///  }
///}
/// ```
///
pub struct OrderTradeUpdatePayload {
    /// 事件类型
    #[serde(rename = "e")]
    pub event: String,

    /// 事件类型时间戳（撮合时间）
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 撮合时间（撮合时间）
    #[serde(rename = "T")]
    pub trade_time: u64,

    /// 订单对象
    #[serde(rename = "o")]
    pub order: Option<OrderInfo>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OrderInfo {
    /// 交易对，例如 "BTCUSDT"
    #[serde(rename = "s")]
    pub symbol: String,

    /// 客户端自定订单ID
    #[serde(rename = "c")]
    pub client_order_id: Option<String>,

    /// 订单方向 BUY/SELL
    #[serde(rename = "S")]
    pub side: Option<String>,

    /// 订单类型，例如 "TRAILING_STOP_MARKET"
    #[serde(rename = "o")]
    pub order_type: String,

    /// 有效方式，例如 "GTC"
    #[serde(rename = "f")]
    pub time_in_force: String,

    /// 订单原始数量（字符串数值）
    #[serde(rename = "q", with = "string_to_decimal")]
    pub quantity: Decimal,

    /// 订单原始价格（字符串数值）
    #[serde(rename = "p", with = "string_to_decimal")]
    pub price: Decimal,

    /// 订单平均价格（字符串数值）
    #[serde(rename = "ap", with = "string_to_decimal")]
    pub avg_price: Decimal,

    /// 条件订单触发价格（字符串数值）
    #[serde(rename = "sp", with = "string_to_decimal")]
    pub stop_price: Decimal,

    /// 本次事件的具体执行类型
    #[serde(rename = "x")]
    pub execution_type: Option<String>,

    /// 订单当前状态
    #[serde(rename = "X")]
    pub current_order_status: Option<String>,

    /// 订单ID
    #[serde(rename = "i")]
    pub order_id: Option<u64>,

    /// 订单末次成交量（字符串数值）
    #[serde(rename = "l", with = "string_to_decimal")]
    pub last_filled_qty: Decimal,

    /// 订单累计已成交量（字符串数值）
    #[serde(rename = "z", with = "string_to_decimal")]
    pub executed_qty: Decimal,

    /// 订单末次成交价格（字符串数值）
    #[serde(rename = "L", with = "string_to_decimal")]
    pub last_filled_price: Decimal,

    /// 手续费资产类型
    #[serde(rename = "N")]
    pub fee_asset: String,

    /// 手续费数量（字符串数值）
    #[serde(rename = "n", with = "string_to_decimal")]
    pub fee: Decimal,

    /// 成交时间（毫秒）
    #[serde(rename = "T")]
    pub trade_time: u64,

    /// 成交ID
    #[serde(rename = "t")]
    pub trade_id: u64,

    /// 买单净值（字符串数值）
    #[serde(rename = "b", with = "string_to_decimal")]
    pub buyer_order_gross: Decimal,

    /// 卖单净值（字符串数值）
    #[serde(rename = "a", with = "string_to_decimal")]
    pub seller_order_gross: Decimal,

    /// 该成交是作为挂单成交吗？
    #[serde(rename = "m")]
    pub is_maker: bool,

    /// 是否是只减仓单
    #[serde(rename = "R")]
    pub is_reduce_only: bool,

    /// 触发价类型
    #[serde(rename = "wt")]
    pub working_type: Option<String>,

    /// 原始订单类型
    #[serde(rename = "ot")]
    pub original_order_type: Option<String>,

    /// 持仓方向
    #[serde(rename = "ps")]
    pub position_side: String,

    /// 是否为触发平仓单; 仅在条件订单情况下会推送此字段
    #[serde(rename = "cp")]
    pub is_close_position: bool,

    /// 追踪止损激活价格（字符串数值）
    #[serde(rename = "AP", with = "string_to_decimal")]
    pub activation_price: Decimal,

    /// 追踪止损回调比例（字符串数值）
    #[serde(rename = "cr", with = "string_to_decimal")]
    pub callback_rate: Decimal,

    /// 是否开启条件单触发保护
    #[serde(rename = "pP")]
    pub trigger_protect: bool,

    /// 忽略字段 si
    #[serde(rename = "si")]
    pub si: Option<i32>,

    /// 忽略字段 ss
    #[serde(rename = "ss")]
    pub ss: Option<i32>,

    /// 该交易实现盈亏（字符串数值）
    #[serde(rename = "rp", with = "string_to_decimal")]
    pub realized_pnl: Decimal,

    /// 自成交防止模式
    #[serde(rename = "V")]
    pub self_trade_prevention_mode: Option<String>,

    /// 价格匹配模式
    #[serde(rename = "pm")]
    pub price_match_mode: Option<String>,

    /// TIF为GTD的订单自动取消时间
    #[serde(rename = "gtd")]
    pub gtd: Option<u64>,

    /// 过期原因
    #[serde(rename = "er")]
    pub expire_reason: Option<String>,
}

/// 精简交易推送
/// [精简交易推送](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/user-data-streams/Event-Trade-Lite)
///
/// ```json
/// {
///   "e":"TRADE_LITE",             // 事件类型
///   "E":1721895408092,            // 事件时间
///   "T":1721895408214,            // 交易时间
///   "s":"BTCUSDT",                // 交易对
///   "q":"0.001",                  // 订单原始数量
///   "p":"0",                      // 订单原始价格
///   "m":false,                    // 该成交是作为挂单成交吗？
///   "c":"z8hcUoOsqEdKMeKPSABslD", // 客户端自定订单ID
///       // 特殊的自定义订单ID:
///       // "autoclose-"开头的字符串: 系统强平订单
///       // "adl_autoclose": ADL自动减仓订单
///       // "settlement_autoclose-": 下架或交割的结算订单
///   "S":"BUY",                    // 订单方向
///   "L":"64089.20",               // 订单末次成交价格
///   "l":"0.040",                  // 订单末次成交量
///   "t":109100866,                // 成交ID
///   "i":8886774,                  // 订单ID
///  }
/// ```
///
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TradeLitePayload {
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间（毫秒）
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 交易时间（毫秒）
    #[serde(rename = "T")]
    pub trade_time: u64,

    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// 订单原始数量（字符串数值）
    #[serde(rename = "q", with = "string_to_decimal")]
    pub quantity: Decimal,

    /// 订单原始价格（字符串数值）
    #[serde(rename = "p", with = "string_to_decimal")]
    pub price: Decimal,

    /// 该成交是作为挂单成交吗？
    #[serde(rename = "m")]
    pub is_maker: bool,

    /// 客户端自定订单ID
    #[serde(rename = "c")]
    pub client_order_id: Option<String>,

    /// 订单方向
    #[serde(rename = "S")]
    pub side: String,

    /// 订单末次成交价格（字符串数值）
    #[serde(rename = "L", with = "string_to_option_decimal")]
    pub last_price: Option<Decimal>,

    /// 订单末次成交量（字符串数值）
    #[serde(rename = "l", with = "string_to_option_decimal")]
    pub last_qty: Option<Decimal>,

    /// 成交ID
    #[serde(rename = "t")]
    pub trade_id: Option<u64>,

    /// 订单ID
    #[serde(rename = "i")]
    pub order_id: Option<u64>,
}

/// 杠杆倍数等账户配置 更新推送
/// [杠杆倍数等账户配置 更新推送](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/user-data-streams/Event-Account-Configuration-Update-previous-Leverage-Update)
///
/// 有两种形式。
///
/// ```json
///{
///     "e":"ACCOUNT_CONFIG_UPDATE",       // 事件类型
///     "E":1611646737479,		           // 事件时间
///     "T":1611646737476,		           // 撮合时间
///     "ac":{
///     "s":"BTCUSDT",					   // 交易对
///     "l":25						       // 杠杆倍数
///     }
/// }
/// ```
/// or
/// ```json
/// {
///     "e":"ACCOUNT_CONFIG_UPDATE",       // 事件类型
///     "E":1611646737479,		           // 事件时间
///     "T":1611646737476,		           // 撮合时间
///     "ai":{							   // 用户账户配置
///     "j":true						   // 联合保证金状态
///     }
/// }
/// ```
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AccountConfigUpdatePayLoad {
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间（毫秒）
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 交易时间（毫秒）
    #[serde(rename = "T")]
    pub trade_time: u64,

    /// 交易对级别的杠杆配置
    #[serde(rename = "ac")]
    pub ac: Option<AccountLeverage>,

    /// 用户账户级别配置
    #[serde(rename = "ai")]
    pub ai: Option<AccountInfo>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AccountLeverage {
    /// 交易对，例如 "BTCUSDT"
    #[serde(rename = "s")]
    pub symbol: String,

    /// 杠杆倍数
    #[serde(rename = "l")]
    pub leverage: i32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AccountInfo {
    /// 联合保证金状态
    #[serde(rename = "j")]
    pub joined_margin: Option<bool>,
}

/// 策略交易更新推送
/// [策略交易更新推送](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/user-data-streams/Event-STRATEGY-UPDATE)
///
/// ```json
/// {
/// 	"e": "STRATEGY_UPDATE", // 事件类型
/// 	"T": 1669261797627, // 撮合时间
/// 	"E": 1669261797628, // 事件时间
/// 	"su": {
/// 			"si": 176054594, // 策略 ID
/// 			"st": "GRID", // 策略类型
/// 			"ss": "NEW", // 策略状态
/// 			"s": "BTCUSDT", // 交易对
/// 			"ut": 1669261797627, // 更新时间
///			"c": 8007 // opCode
/// 		}
/// }
/// ```
///
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StrategyUpdatePayLoad {
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间（毫秒）
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 交易时间（毫秒）
    #[serde(rename = "T")]
    pub trade_time: u64,

    /// 策略更新对象
    #[serde(rename = "su")]
    pub strategy_update: Option<StrategyUpdate>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StrategyUpdate {
    /// 策略 ID
    #[serde(rename = "si")]
    pub strategy_id: u64,

    /// 策略类型，例如 "GRID"
    #[serde(rename = "st")]
    pub strategy_type: String,

    /// 策略状态，例如 "NEW"
    #[serde(rename = "ss")]
    pub status: String,

    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// 更新时间（毫秒）
    #[serde(rename = "ut")]
    pub updated_time: u64,

    /// opCode
    #[serde(rename = "c")]
    pub opcode: Option<i32>,
}

/// 网格更新推送
/// [网格更新推送](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/user-data-streams/Event-GRID-UPDATE)
///
/// ```json
/// {
/// 	"e": "GRID_UPDATE", // 事件类型
/// 	"T": 1669262908216, // 撮合时间
/// 	"E": 1669262908218, // 事件时间
/// 	"gu": {
/// 			"si": 176057039, // 策略 ID
/// 			"st": "GRID", // 策略类型
/// 			"ss": "WORKING", // 策略状态
/// 			"s": "BTCUSDT", // 交易对
/// 			"r": "-0.00300716", // 已实现 PNL
/// 			"up": "16720", // 未配对均价
/// 			"uq": "-0.001", // 未配对数量
/// 			"uf": "-0.00300716", // 未配对手续费
/// 			"mp": "0.0", // 已配对 PNL
/// 			"ut": 1669262908197 // 更新时间
/// 		}
/// }
/// ```
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GridUpdatePayLoad {
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间（毫秒）
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 交易时间（毫秒）
    #[serde(rename = "T")]
    pub trade_time: u64,

    /// 网格更新对象
    #[serde(rename = "gu")]
    pub grid_update: Option<GridUpdate>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GridUpdate {
    /// 策略 ID
    #[serde(rename = "si")]
    pub strategy_id: u64,

    /// 策略类型
    #[serde(rename = "st")]
    pub strategy_type: String,

    /// 策略状态
    #[serde(rename = "ss")]
    pub status: String,

    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// 已实现 PNL（字符串数值）
    #[serde(rename = "r", with = "string_to_option_decimal")]
    pub realized_pnl: Option<Decimal>,

    /// 未配对均价（字符串数值）
    #[serde(rename = "up", with = "string_to_option_decimal")]
    pub unmatched_price: Option<Decimal>,

    /// 未配对数量（字符串数值）
    #[serde(rename = "uq", with = "string_to_option_decimal")]
    pub unmatched_qty: Option<Decimal>,

    /// 未配对手续费（字符串数值）
    #[serde(rename = "uf", with = "string_to_option_decimal")]
    pub unmatched_fee: Option<Decimal>,

    /// 已配对 PNL（字符串数值）
    #[serde(rename = "mp", with = "string_to_option_decimal")]
    pub matched_pnl: Option<Decimal>,

    /// 更新时间（毫秒）
    #[serde(rename = "ut")]
    pub updated_time: Option<u64>,
}

/// 条件订单(TP/SL)触发后拒绝更新推送
/// [条件订单(TP/SL)触发后拒绝更新推送](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/user-data-streams/Event-Conditional-Order-Trigger-Reject)
///
/// ```json
/// {
///     "e":"CONDITIONAL_ORDER_TRIGGER_REJECT",      // 事件类型
///     "E":1685517224945,      // 事件时间
///     "T":1685517224955,      // 撮合时间
///     "or":{
///       "s":"ETHUSDT",      // 交易对
///       "i":155618472834,      // 订单号
///       "r":"Due to the order could not be filled immediately, the FOK order has been rejected. The order will not be recorded in the order history",      // 拒绝原因
///      }
/// }
/// ```
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ConditionalOrderTriggerRejectPayLoad {
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间（毫秒）
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 交易时间（毫秒）
    #[serde(rename = "T")]
    pub trade_time: u64,

    /// 被拒绝的订单信息
    #[serde(rename = "or")]
    pub order_reject: Option<ConditionalRejectOrder>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ConditionalRejectOrder {
    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// 订单号
    #[serde(rename = "i")]
    pub order_id: u64,

    /// 拒绝原因
    #[serde(rename = "r")]
    pub reason: Option<String>,
}

/// 条件订单交易更新推送
/// [条件订单交易更新推送](https://developers.binance.com/docs/zh-CN/derivatives/usds-margined-futures/user-data-streams/Event-Algo-Order-Update)
///
/// ''‘json
////{
///   "e":"ALGO_UPDATE",  // 事件类型
///   "T":1750515742297,  // 撮合时间
///   "E":1750515742303,  // 事件时间
///   "o":{
///     "caid":"Q5xaq5EGKgXXa0fD7fs0Ip",  // 客户端自定条件订单ID
///     "aid":2148719,  // 条件单 Id
///     "at":"CONDITIONAL",  // 条件单类型
///     "o":"TAKE_PROFIT",  //订单类型
///     "s":"BNBUSDT",  //交易对
///     "S":"SELL",  //订单方向
///     "ps":"BOTH",  //持仓方向
///     "f":"GTC",  //有效方式
///     "q":"0.01",  //订单数量
///     "X":"CANCELED",  //条件单状态
///     "ai":"",  // 触发后普通订单 id
///     "ap": "0.00000", // 触发后在撮合引擎中实际订单的平均成交价格，仅在订单被触发并进入撮合引擎时显示
///     "aq": "0.00000", // 触发后在撮合引擎中实际订单已成交数量，仅当订单被触发并进入撮合引擎时显示
///     "act": "0", // 触发后在撮合引擎中实际的订单类型，仅当订单被触发并进入撮合引擎时显示
///     "tp":"750",  //条件单触发价格
///     "p":"750", //订单价格
///     "V":"EXPIRE_MAKER",  //自成交防止模式
///     "wt":"CONTRACT_PRICE", //触发价类型
///     "pm":"NONE",  // 价格匹配模式
///     "cp":false,  //是否为触发平仓单; 仅在条件订单情况下会推送此字段
///     "pP":false, //是否开启条件单触发保护
///     "R":false,  // 是否是只减仓单
///     "tt":0,  //触发时间
///     "gtd":0,       // TIF为GTD的订单自动取消时间
///     "rm": "Reduce Only reject"  // 条件单失败原因
/// }
/// ```
///
///
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AlgoUpdatePayload {
    #[serde(rename = "e")]
    pub event: String,

    /// 事件时间（毫秒）
    #[serde(rename = "E")]
    pub event_time: u64,

    /// 交易时间（毫秒）
    #[serde(rename = "T")]
    pub trade_time: u64,

    /// 条件单对象
    #[serde(rename = "o")]
    pub order: Option<AlgoOrderInfo>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AlgoOrderInfo {
    /// 客户端自定条件订单ID
    #[serde(rename = "caid")]
    pub client_algo_id: Option<String>,

    /// 条件单 Id
    #[serde(rename = "aid")]
    pub algo_id: u64,

    /// 条件单类型
    #[serde(rename = "at")]
    pub algo_type: String,

    /// 订单类型
    #[serde(rename = "o")]
    pub order_type: String,

    /// 交易对
    #[serde(rename = "s")]
    pub symbol: String,

    /// 订单方向
    #[serde(rename = "S")]
    pub side: String,

    /// 持仓方向
    #[serde(rename = "ps")]
    pub position_side: String,

    /// 有效方式
    #[serde(rename = "f")]
    pub time_in_force: Option<String>,

    /// 订单数量（字符串数值）
    #[serde(rename = "q", with = "string_to_decimal")]
    pub quantity: Decimal,

    /// 条件单状态
    #[serde(rename = "X")]
    pub status: String,

    /// 触发后普通订单 id
    #[serde(rename = "ai")]
    pub after_order_id: Option<String>,

    /// 触发后在撮合引擎中实际订单的平均成交价格（字符串数值）
    #[serde(rename = "ap", with = "string_to_decimal")]
    pub avg_price: Decimal,

    /// 触发后在撮合引擎中实际订单已成交数量（字符串数值）
    #[serde(rename = "aq", with = "string_to_decimal")]
    pub executed_qty: Decimal,

    /// 触发后在撮合引擎中实际的订单类型
    #[serde(rename = "act")]
    pub actual_type: Option<String>,

    /// 条件单触发价格（字符串数值）
    #[serde(rename = "tp", with = "string_to_option_decimal")]
    pub trigger_price: Option<Decimal>,

    /// 订单价格（字符串数值）
    #[serde(rename = "p", with = "string_to_decimal")]
    pub price: Decimal,

    /// 自成交防止模式
    #[serde(rename = "V")]
    pub self_trade_prevention_mode: Option<String>,

    /// 触发价类型
    #[serde(rename = "wt")]
    pub working_type: Option<String>,

    /// 价格匹配模式
    #[serde(rename = "pm")]
    pub price_match_mode: Option<String>,

    /// 是否为触发平仓单
    #[serde(rename = "cp")]
    pub is_close_position: Option<bool>,

    /// 是否开启条件单触发保护
    #[serde(rename = "pP")]
    pub trigger_protect: Option<bool>,

    /// 是否是只减仓单
    #[serde(rename = "R")]
    pub is_reduce_only: Option<bool>,

    /// 触发时间（毫秒）
    #[serde(rename = "tt")]
    pub trigger_time: Option<u64>,

    /// TIF为GTD的订单自动取消时间
    #[serde(rename = "gtd")]
    pub gtd: Option<u64>,

    /// 条件单失败原因
    #[serde(rename = "rm")]
    pub reject_msg: Option<String>,
}
