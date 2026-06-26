use chrono::{DateTime, Utc};
use serde::Deserializer;
use serde::de;
use serde::{Deserialize, Serialize};
use serde_json::Value;
/**
[series schema](https://gamma-api.polymarket.com/schemas/Series.json)
[event schema](https://gamma-api.polymarket.com/schemas/Event.json)
[market schema](https://gamma-api.polymarket.com/schemas/Market.json)
**/

// 将可选的 RFC3339 字符串反序列化为 Unix timestamp (seconds, UTC)
fn de_opt_rfc3339_to_unix<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    match opt {
        None => Ok(None),
        Some(s) => {
            let s_trim = s.trim();
            if s_trim.is_empty() {
                return Ok(None);
            }
            match DateTime::parse_from_rfc3339(s_trim) {
                Ok(dt) => Ok(Some(dt.with_timezone(&Utc).timestamp() as u64)),
                Err(e) => Err(de::Error::custom(format!("invalid datetime '{}': {}", s_trim, e))),
            }
        }
    }
}

// 下面两个函数用于反序列化那些以字符串形式包装的 JSON 数组
fn de_opt_string_or_vec_string<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<Value> = Option::deserialize(deserializer)?;
    match opt {
        None => Ok(None),
        Some(Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                match v {
                    Value::String(s) => out.push(s),
                    other => out.push(other.to_string()),
                }
            }
            Ok(Some(out))
        }
        Some(Value::String(s)) => {
            let s_trim = s.trim();
            if s_trim.is_empty() {
                return Ok(None);
            }
            // 先尝试当作 JSON array 的字符串解析
            if let Ok(vec) = serde_json::from_str::<Vec<String>>(s_trim) {
                return Ok(Some(vec));
            }
            // 否则尝试按逗号分割
            let vec: Vec<String> = s_trim
                .trim_matches(|c| c == '[' || c == ']')
                .split(',')
                .map(|p| p.trim().trim_matches('"').to_string())
                .filter(|p| !p.is_empty())
                .collect();
            Ok(Some(vec))
        }
        Some(other) => {
            // 其它类型，尝试转换为单元素字符串数组
            Ok(Some(vec![other.to_string()]))
        }
    }
}

fn de_opt_string_or_vec_f64<'de, D>(deserializer: D) -> Result<Option<Vec<f64>>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<Value> = Option::deserialize(deserializer)?;
    match opt {
        None => Ok(None),
        Some(Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                match v {
                    Value::Number(n) => {
                        if let Some(f) = n.as_f64() {
                            out.push(f);
                        } else {
                            return Err(de::Error::custom("invalid number"));
                        }
                    }
                    Value::String(s) => {
                        let parsed = s.parse::<f64>().map_err(|e| de::Error::custom(format!("parse float: {}", e)))?;
                        out.push(parsed);
                    }
                    other => {
                        let parsed = other
                            .to_string()
                            .parse::<f64>()
                            .map_err(|e| de::Error::custom(format!("parse float: {}", e)))?;
                        out.push(parsed);
                    }
                }
            }
            Ok(Some(out))
        }
        Some(Value::String(s)) => {
            let s_trim = s.trim();
            if s_trim.is_empty() {
                return Ok(None);
            }
            if let Ok(vec) = serde_json::from_str::<Vec<f64>>(s_trim) {
                return Ok(Some(vec));
            }
            // 否则按逗号分割并解析为 f64
            let vec_res: Result<Vec<f64>, _> = s_trim
                .trim_matches(|c| c == '[' || c == ']')
                .split(',')
                .map(|p| p.trim().trim_matches('"').parse::<f64>())
                .collect();
            let vec = vec_res.map_err(|e| de::Error::custom(format!("parse float list: {}", e)))?;
            Ok(Some(vec))
        }
        Some(other) => {
            // 单值，尝试解析为 f64
            let parsed = other
                .to_string()
                .trim_matches('"')
                .parse::<f64>()
                .map_err(|e| de::Error::custom(format!("parse float: {}", e)))?;
            Ok(Some(vec![parsed]))
        }
    }
}

/// Polymarket: Market / Event / Series 模型，基于 schema JSON
/// 许多复杂或未明确定义的子对象使用 serde_json::Value 保留原始结构

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Market {
    pub id: String,

    // 标识符与状态
    pub accepting_orders: Option<bool>,
    pub accepting_orders_timestamp: Option<String>,
    pub active: Option<bool>,
    pub amm_type: Option<String>,
    pub approved: Option<bool>,
    pub archived: Option<bool>,
    pub automatically_active: Option<bool>,
    pub automatically_resolved: Option<bool>,

    // 价格/流动性
    pub best_ask: Option<f64>,
    pub best_bid: Option<f64>,

    // 分类与显示
    pub category: Option<String>,
    pub category_mailchimp_tag: Option<String>,
    pub chart_color: Option<String>,

    // 交易规则与费率
    pub clear_book_on_start: Option<bool>,
    pub clob_rewards: Option<Vec<Value>>,
    #[serde(default, deserialize_with = "de_opt_string_or_vec_string")]
    pub clob_token_ids: Option<Vec<String>>,
    pub closed: Option<bool>,
    pub closed_time: Option<String>,
    pub combo_status: Option<String>,
    pub comments_enabled: Option<bool>,
    pub competitive: Option<f64>,
    pub condition_id: String,
    pub created_at: Option<String>,
    pub created_by: Option<i64>,
    pub creator: Option<String>,
    pub curation_order: Option<i64>,
    pub custom_liveness: Option<i64>,
    pub cyom: Option<bool>,

    // 代币/资金相关
    pub denomination_token: Option<String>,
    pub deploying: Option<bool>,
    pub deploying_timestamp: Option<String>,

    // 描述与外观
    pub description: Option<String>,
    pub disqus_thread: Option<String>,

    // 订单簿配置
    pub enable_order_book: Option<bool>,
    #[serde(default, deserialize_with = "de_opt_rfc3339_to_unix")]
    pub end_date: Option<u64>,
    pub end_date_iso: Option<String>,
    pub event_start_time: Option<String>,

    // 关联对象
    pub events: Option<Vec<Event>>,
    pub featured: Option<bool>,

    // 费用/费率
    pub fee: Option<String>,
    pub fee_exponent: Option<f64>,
    pub fee_rate: Option<f64>,
    pub fee_schedule: Option<Value>,
    pub fee_type: Option<String>,
    pub fees_enabled: Option<bool>,

    pub format_type: Option<String>,
    pub fpmm_live: Option<bool>,
    pub funded: Option<bool>,
    pub funded_timestamp: Option<String>,
    pub game_id: Option<String>,
    pub game_start_time: Option<String>,

    // 分组相关
    pub group_item_range: Option<String>,
    pub group_item_threshold: Option<String>,
    pub group_item_title: Option<String>,
    pub has_reviewed_dates: Option<bool>,
    pub holding_rewards_enabled: Option<bool>,

    pub icon: Option<String>,
    pub icon_optimized: Option<Value>,
    pub image: Option<String>,
    pub image_optimized: Option<Value>,

    pub internal_users: Option<Vec<Value>>,

    // 交易相关数值
    pub last_trade_price: Option<f64>,
    pub line: Option<f64>,
    pub liquidity: Option<String>,
    pub liquidity_amm: Option<f64>,
    pub liquidity_clob: Option<f64>,
    pub liquidity_num: Option<f64>,

    pub lower_bound: Option<String>,
    pub lower_bound_date: Option<String>,
    pub mailchimp_tag: Option<String>,

    pub maker_base_fee: Option<i64>,
    pub maker_rebates_fee_share_bps: Option<i64>,
    pub manual_activation: Option<bool>,
    pub market_group: Option<i64>,
    pub market_maker_address: String,
    pub market_metadata: Option<Value>,
    pub market_type: Option<String>,

    // 嵌套 markets（可能的组合市场）
    pub markets: Option<Vec<Value>>,

    pub neg_risk: Option<bool>,
    pub neg_risk_market_id: Option<String>,
    pub neg_risk_other: Option<bool>,
    pub neg_risk_request_id: Option<String>,

    pub new: Option<bool>,
    pub notifications_enabled: Option<bool>,

    // 价格变动
    pub one_day_price_change: Option<f64>,
    pub one_hour_price_change: Option<f64>,
    pub one_month_price_change: Option<f64>,
    pub one_week_price_change: Option<f64>,
    pub one_year_price_change: Option<f64>,

    pub order_min_size: Option<f64>,
    pub order_price_min_tick_size: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_string_or_vec_f64")]
    pub outcome_prices: Option<Vec<f64>>,
    #[serde(default, deserialize_with = "de_opt_string_or_vec_string")]
    pub outcomes: Option<Vec<String>>,

    pub pager_duty_notification_enabled: Option<bool>,
    pub past_slugs: Option<String>,
    pub pending_deployment: Option<bool>,
    pub position_ids: Option<Vec<String>>,
    pub question: Option<String>,
    pub question_id: Option<String>,
    pub ready: Option<bool>,
    pub ready_for_cron: Option<bool>,
    pub ready_timestamp: Option<String>,
    pub requires_translation: Option<bool>,
    pub resolution_source: Option<String>,
    pub resolved_by: Option<String>,
    pub restricted: Option<bool>,
    pub rewards_max_spread: Option<f64>,
    pub rewards_min_size: Option<f64>,
    pub rfq_enabled: Option<bool>,
    pub scheduled_deployment_timestamp: Option<String>,
    pub score: Option<i64>,
    pub seconds_delay: Option<i64>,
    pub sent_discord: Option<bool>,
    pub series_color: Option<String>,
    pub short_outcomes: Option<String>,
    pub show_gmp_outcome: Option<bool>,
    pub show_gmp_series: Option<bool>,
    pub slug: String,
    pub sponsor_image: Option<String>,
    pub sponsor_name: Option<String>,
    pub sports_market_type: Option<String>,
    pub spread: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_rfc3339_to_unix")]
    pub start_date: Option<u64>,
    pub start_date_iso: Option<String>,
    pub subcategory: Option<String>,
    pub submitted_by: Option<String>,
    pub tags: Option<Vec<Value>>,
    pub taker_base_fee: Option<i64>,
    pub team_a_id: Option<String>,
    pub team_b_id: Option<String>,
    pub twitter_card_image: Option<String>,
    pub twitter_card_last_refreshed: Option<String>,
    pub twitter_card_last_validated: Option<String>,
    pub twitter_card_location: Option<String>,
    pub uma_bond: Option<String>,
    pub uma_end_date: Option<String>,
    pub uma_end_date_iso: Option<String>,
    pub uma_resolution_status: Option<String>,
    pub uma_resolution_statuses: Option<String>,
    pub uma_reward: Option<String>,
    pub updated_at: Option<String>,
    pub updated_by: Option<i64>,
    pub upper_bound: Option<String>,
    pub upper_bound_date: Option<String>,

    // 体积字段
    pub volume: Option<String>,
    pub volume1mo: Option<f64>,
    pub volume1mo_amm: Option<f64>,
    pub volume1mo_clob: Option<f64>,
    pub volume1wk: Option<f64>,
    pub volume1wk_amm: Option<f64>,
    pub volume1wk_clob: Option<f64>,
    pub volume1yr: Option<f64>,
    pub volume1yr_amm: Option<f64>,
    pub volume1yr_clob: Option<f64>,
    pub volume24hr: Option<f64>,
    pub volume24hr_amm: Option<f64>,
    pub volume24hr_clob: Option<f64>,
    pub volume_amm: Option<f64>,
    pub volume_clob: Option<f64>,
    pub volume_num: Option<f64>,
    pub wide_format: Option<bool>,
    pub x_axis_value: Option<String>,
    pub y_axis_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,

    pub active: Option<bool>,
    pub archived: Option<bool>,
    pub automatically_active: Option<bool>,
    pub automatically_resolved: Option<bool>,
    pub best_lines: Option<Vec<Value>>,
    pub cant_estimate: Option<bool>,
    pub carousel_map: Option<String>,
    pub category: Option<String>,
    pub closed: Option<bool>,
    pub closed_time: Option<String>,
    pub collections: Option<Vec<Value>>,
    pub color: Option<String>,
    pub comment_count: Option<i64>,
    pub comments_enabled: Option<bool>,
    pub competitive: Option<f64>,
    pub country_name: Option<String>,
    pub created_at: Option<String>,
    pub created_by: Option<String>,
    pub creation_date: Option<String>,
    pub cumulative_markets: Option<bool>,
    pub cyom: Option<bool>,
    pub deploying: Option<bool>,
    pub deploying_timestamp: Option<String>,
    pub description: Option<String>,
    pub disqus_thread: Option<String>,
    pub elapsed: Option<String>,
    pub election_type: Option<String>,
    pub enable_neg_risk: Option<bool>,
    pub enable_order_book: Option<bool>,
    #[serde(default, deserialize_with = "de_opt_rfc3339_to_unix")]
    pub end_date: Option<u64>,
    pub ended: Option<bool>,
    pub estimate_value: Option<bool>,
    pub estimated_value: Option<String>,
    pub event_creators: Option<Vec<Value>>,
    pub event_date: Option<String>,
    pub event_metadata: Option<Value>,
    pub event_week: Option<i64>,
    pub external_partners: Option<Vec<Value>>,
    pub featured: Option<bool>,
    pub featured_image: Option<String>,
    pub featured_image_optimized: Option<Value>,
    pub featured_order: Option<i64>,
    pub finished_timestamp: Option<String>,
    pub game_id: Option<i64>,
    pub gmp_chart_mode: Option<String>,
    pub icon: Option<String>,
    pub icon_optimized: Option<Value>,
    pub image: Option<String>,
    pub image_optimized: Option<Value>,
    pub internal_users: Option<Vec<Value>>,
    pub is_template: Option<bool>,
    pub last_highlight: Option<String>,
    pub last_highlight_at: Option<String>,
    pub last_highlight_type: Option<String>,
    pub liquidity: Option<f64>,
    pub liquidity_amm: Option<f64>,
    pub liquidity_clob: Option<f64>,
    pub live: Option<bool>,
    pub markets: Option<Vec<Market>>,
    pub neg_risk: Option<bool>,
    pub neg_risk_augmented: Option<bool>,
    pub neg_risk_fee_bips: Option<i64>,
    pub neg_risk_market_id: Option<String>,
    pub new: Option<bool>,
    pub open_interest: Option<f64>,
    pub parent_event_id: Option<i64>,
    pub pending_deployment: Option<bool>,
    pub period: Option<String>,
    pub published_at: Option<String>,
    pub requires_translation: Option<bool>,
    pub rescheduled_from_game_id: Option<i64>,
    pub resolution_source: Option<String>,
    pub restricted: Option<bool>,
    pub scheduled_deployment_timestamp: Option<String>,
    pub score: Option<String>,
    pub series: Option<Vec<Series>>,
    pub series_slug: Option<String>,
    pub show_all_outcomes: Option<bool>,
    pub show_market_images: Option<bool>,
    pub slug: String,
    pub sort_by: Option<String>,
    pub sport: Option<Value>,
    pub spreads_main_line: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_rfc3339_to_unix")]
    pub start_date: Option<u64>,
    pub start_time: Option<String>,
    pub sub_events: Option<Vec<Option<String>>>,

    pub subcategory: Option<String>,
    pub subtitle: Option<String>,
    pub tag_labels: Option<Vec<String>>,
    pub tag_slugs: Option<Vec<String>>,
    pub tags: Option<Vec<Value>>,
    pub teams: Option<Vec<Value>>,
    pub template_variables: Option<String>,
    pub templates: Option<Vec<Value>>,
    pub ticker: Option<String>,
    pub title: Option<String>,
    pub totals_main_line: Option<f64>,
    pub turn_provider_id: Option<String>,
    pub tweet_count: Option<i64>,
    pub updated_at: Option<String>,
    pub updated_by: Option<String>,
    pub volume: Option<f64>,
    pub volume1mo: Option<f64>,
    pub volume1wk: Option<f64>,
    pub volume1yr: Option<f64>,
    pub volume24hr: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Series {
    pub id: String,
    pub ticker: Option<String>,
    pub slug: String,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub series_type: Option<String>,
    pub recurrence: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub icon: Option<String>,
    pub layout: Option<String>,
    pub active: Option<bool>,
    pub closed: Option<bool>,
    pub archived: Option<bool>,
    #[serde(rename = "new")]
    pub is_new: Option<bool>,
    pub featured: Option<bool>,
    pub restricted: Option<bool>,
    pub is_template: Option<bool>,
    pub template_variables: Option<bool>,
    pub published_at: Option<String>,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub comments_enabled: Option<bool>,
    pub competitive: Option<String>,
    pub volume24hr: Option<f64>,
    pub volume: Option<f64>,
    pub liquidity: Option<f64>,
    #[serde(default, deserialize_with = "de_opt_rfc3339_to_unix")]
    pub start_date: Option<u64>,
    pub pyth_token_id: Option<String>,
    pub cg_asset_name: Option<String>,
    pub score: Option<i64>,
    pub events: Option<Vec<Event>>,
    pub collections: Option<Vec<Value>>,
    pub categories: Option<Vec<Value>>,
    pub tags: Option<Vec<Value>>,
    pub comment_count: Option<i64>,
    pub chats: Option<Vec<Value>>,
}

/// Polymarket CLOB: GET /prices-history
/// 文档: https://docs.polymarket.com/api-reference/markets/get-prices-history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketPriceHistoryPoint {
    /// Unix 时间戳（秒）
    pub t: u64,
    /// 该时间点价格
    pub p: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPricesHistoryResponse {
    pub history: Vec<MarketPriceHistoryPoint>,
}

/// Polymarket CLOB: GET /prices-history 查询参数
/// 文档: https://docs.polymarket.com/api-reference/markets/get-prices-history
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPricesHistoryQuery {
    /// 市场 token ID (必须)
    pub market: String,
    /// 开始时间戳 (Unix seconds, 可选)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_ts: Option<u64>,
    /// 结束时间戳 (Unix seconds, 可选)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_ts: Option<u64>,
    /// 时间间隔: max, 1w, 1d, 6h, 1h (可选)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<String>,
    /// 精度 (分钟, 默认 1, 可选)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fidelity: Option<u32>,
}

impl GetPricesHistoryQuery {
    pub fn new(market: String) -> Self {
        Self {
            market,
            start_ts: None,
            end_ts: None,
            interval: None,
            fidelity: None,
        }
    }

    pub fn start_ts(mut self, ts: u64) -> Self {
        self.start_ts = Some(ts);
        self
    }

    pub fn end_ts(mut self, ts: u64) -> Self {
        self.end_ts = Some(ts);
        self
    }

    pub fn interval(mut self, interval: &str) -> Self {
        self.interval = Some(interval.to_string());
        self
    }

    pub fn fidelity(mut self, fidelity: u32) -> Self {
        self.fidelity = Some(fidelity);
        self
    }

    /// 转换为 query string，用于拼接到 URL
    pub fn to_query_string(&self) -> String {
        let mut params = vec![format!("market={}", self.market)];
        if let Some(ts) = self.start_ts {
            params.push(format!("startTs={}", ts));
        }
        if let Some(ts) = self.end_ts {
            params.push(format!("endTs={}", ts));
        }
        if let Some(ref interval) = self.interval {
            params.push(format!("interval={}", interval));
        }
        if let Some(fidelity) = self.fidelity {
            params.push(format!("fidelity={}", fidelity));
        }
        params.join("&")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;
    use chrono::Utc;
    use serde_json::from_value;
    use serde_json::json;

    #[test]
    fn test_market_dates_deserialize() {
        let js = json!({
            "id": "m1",
            "conditionId": "c1",
            "slug":"mm",
            "marketMakerAddress": "addr",
            "endDate": "2026-04-27T04:02:44.780078Z",
            "startDate": "2026-04-27T03:00:00Z"
        });
        let m: Market = from_value(js).expect("deserialize market");
        let expected_end = DateTime::parse_from_rfc3339("2026-04-27T04:02:44.780078Z")
            .unwrap()
            .with_timezone(&Utc)
            .timestamp() as u64;
        let expected_start = DateTime::parse_from_rfc3339("2026-04-27T03:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
            .timestamp() as u64;
        assert_eq!(m.end_date, Some(expected_end));
        assert_eq!(m.start_date, Some(expected_start));
    }

    #[test]
    fn test_event_dates_deserialize_and_optional() {
        let js = json!({
            "id": "e1",
            "endDate": null,
            "slug":"mm",
            "startDate": "2026-05-01T12:34:56.123Z"
        });
        let e: Event = from_value(js).expect("deserialize event");
        assert_eq!(e.end_date, None);
        let expected = DateTime::parse_from_rfc3339("2026-05-01T12:34:56.123Z")
            .unwrap()
            .with_timezone(&Utc)
            .timestamp() as u64;
        assert_eq!(e.start_date, Some(expected));
    }

    #[test]
    fn test_series_dates_empty_string() {
        let js = json!({
            "id": "s1",
            "slug":"mm",
            "startDate": ""
        });
        let s: Series = from_value(js).expect("deserialize series");
        assert_eq!(s.start_date, None);
    }

    #[test]
    fn test_invalid_date_fails() {
        let js = json!({
            "id": "m2",
            "slug":"mm",
            "conditionId": "c2",
            "marketMakerAddress": "addr",
            "endDate": "not-a-date"
        });
        let res: Result<Market, _> = from_value(js);
        assert!(res.is_err());
    }

    #[test]
    fn test_parse_stringified_array_fields() {
        let js = json!({
            "id": "m3",
            "slug":"mm",
            "conditionId": "c3",
            "marketMakerAddress": "addr",
            "outcomes": "[\"Yes\", \"No\"]",
            "outcomePrices": "[\"0\", \"1\"]",
            "clobTokenIds": "[\"90175867192793125945663838766557051528279598195750042448673781121878588612906\", \"59783021789136533842054040384185655763342958046355826158728150853544340844117\"]"
        });
        let m: Market = from_value(js).expect("deserialize market stringified arrays");
        assert_eq!(m.outcomes.unwrap(), vec!["Yes".to_string(), "No".to_string()]);
        let prices = m.outcome_prices.unwrap();
        assert_eq!(prices, vec![0.0_f64, 1.0_f64]);
        let ids = m.clob_token_ids.unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids[0].starts_with("9017586"));
    }

    #[test]
    fn test_parse_actual_array_fields() {
        let js = json!({
            "id": "m4",
            "slug":"mm",
            "conditionId": "c4",
            "marketMakerAddress": "addr",
            "outcomes": ["A","B"],
            "outcomePrices": [0.5, 1.5],
            "clobTokenIds": ["t1","t2"]
        });
        let m: Market = from_value(js).expect("deserialize market arrays");
        assert_eq!(m.outcomes.unwrap(), vec!["A".to_string(), "B".to_string()]);
        let prices = m.outcome_prices.unwrap();
        assert_eq!(prices, vec![0.5_f64, 1.5_f64]);
        let ids = m.clob_token_ids.unwrap();
        assert_eq!(ids, vec!["t1".to_string(), "t2".to_string()]);
    }

    #[test]
    fn test_get_prices_history_response_deserialize() {
        let js = json!({
            "history": [
                { "t": 1710000000, "p": 0.42 },
                { "t": 1710000060, "p": 0.43 }
            ]
        });
        let resp: GetPricesHistoryResponse = from_value(js).expect("deserialize prices history");
        assert_eq!(resp.history.len(), 2);
        assert_eq!(resp.history[0].t, 1710000000);
        assert_eq!(resp.history[0].p, 0.42_f64);
        assert_eq!(resp.history[1].t, 1710000060);
        assert_eq!(resp.history[1].p, 0.43_f64);
    }

    #[test]
    fn test_get_prices_history_query_builder() {
        let query = GetPricesHistoryQuery::new("token123".to_string())
            .start_ts(1710000000)
            .end_ts(1710003600)
            .interval("1h")
            .fidelity(60);
        let qs = query.to_query_string();
        assert!(qs.contains("market=token123"));
        assert!(qs.contains("startTs=1710000000"));
        assert!(qs.contains("endTs=1710003600"));
        assert!(qs.contains("interval=1h"));
        assert!(qs.contains("fidelity=60"));
    }
}
