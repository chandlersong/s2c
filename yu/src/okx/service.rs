use crate::duck_db::DuckDBDSProvider;
use crate::okx::duck_po::OkxKlinePo;
use rust_decimal::prelude::ToPrimitive;
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use yue::models::HistoryInterval;
use yue::okx::restful_api::{InstrumentsParam, OKxApi, default_okx_api};
use yue::query_message::DataSourceProviderTrait;

///
/// 主要是刷新inst_ids。因为inst_ids是会变的。这个是需要维护的。
///
/// 1. 从表中，抽出OKX_INSTRUMENTS，抽出所有的instId。
/// 2. 然后调用list_okx_option方法，family分别为BTC-USD和ETH-USD,调用两次
/// 3. 比对刚才的instId，如果存在，则跳过。
/// 3. 如果不存在，则往数据库里面插入一条数据。
/// 4, 更新把status为live的，更新属性的inst_ids
///
///
///
///
// Helper: fetch instruments for given param, upsert full fields, return newly inserted instIds
pub async fn fetch_and_upsert_instruments(param: InstrumentsParam, duck_db_provider: &DuckDBDSProvider, api: &OKxApi) -> Vec<String> {
    use std::collections::HashSet;
    let mut new_ids: Vec<String> = Vec::new();

    // acquire connection
    let conn_res = duck_db_provider.acquire();
    let conn = match conn_res {
        Ok(c) => c,
        Err(e) => {
            log::error!("acquire connection error when fetch instruments: {:?}", e);
            return new_ids;
        }
    };

    // ensure table exists
    if let Err(e) = conn.execute_batch(crate::okx::duckdb_consts::CREATE_OKX_INSTRUMENTS_TABLE) {
        log::error!("failed to ensure okx instruments table: {:?}", e);
        return new_ids;
    }

    // read existing inst ids and states
    let mut existing: HashSet<String> = HashSet::new();
    let mut existing_state: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT instId, state FROM OKX_INSTRUMENTS;") {
        if let Ok(mut rows) = stmt.query([]) {
            loop {
                match rows.next() {
                    Ok(Some(row)) => {
                        if let Ok(id) = row.get::<usize, String>(0) {
                            let st: String = row.get(1).unwrap_or_default();
                            existing.insert(id.clone());
                            existing_state.insert(id, st);
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        log::error!("error iterating rows: {:?}", e);
                        break;
                    }
                }
            }
        }
    }

    // call API
    match api.list_instruments(param).await {
        Ok(resp) => {
            for info in resp.data.into_iter() {
                let id = info.inst_id.clone();

                // helpers
                let esc = |s: &str| s.replace('\'', "''");
                let q_str = |o: &Option<String>| match o {
                    Some(v) => format!("'{}'", esc(v)),
                    None => "NULL".to_string(),
                };

                let base_ccy = esc(&info.base_ccy);
                let quote_ccy = q_str(&info.quote_ccy);
                let settle_ccy = q_str(&info.settle_ccy);
                let list_time = q_str(&info.list_time);
                let exp_time = match info.exp_time {
                    Some(v) => format!("'{}'", v),
                    None => "NULL".to_string(),
                };
                // convert to f64 when possible
                let tick_sz_val: Option<f64> = info.tick_sz.as_ref().and_then(|d| d.to_f64());
                let lot_sz_val: Option<f64> = info.lot_sz.as_ref().and_then(|d| d.to_f64());
                let min_sz_val: Option<f64> = info.min_sz.as_ref().and_then(|d| d.to_f64());
                let tick_sz = match tick_sz_val {
                    Some(f) => f.to_string(),
                    None => "NULL".to_string(),
                };
                let lot_sz = match lot_sz_val {
                    Some(f) => f.to_string(),
                    None => "NULL".to_string(),
                };
                let min_sz = match min_sz_val {
                    Some(f) => f.to_string(),
                    None => "NULL".to_string(),
                };

                let alias = q_str(&info.alias);
                let state = q_str(&info.state);
                let inst_id_code = match info.inst_id_code {
                    Some(v) => v.to_string(),
                    None => "NULL".to_string(),
                };
                let inst_category = q_str(&info.inst_category);
                let inst_type = esc(&info.inst_type);
                let inst_family = q_str(&info.inst_family);

                if !existing.contains(&id) {
                    // insert full row
                    let insert_sql = format!(
                        "INSERT INTO OKX_INSTRUMENTS(instId, instType, instFamily, baseCcy, quoteCcy, settleCcy, listTime, expTime, tickSz, lotSz, minSz, alias, state, instIdCode, instCategory) VALUES ('{}','{}',{},'{}',{},{},{},{},{},{},{},{},{} ,{},{});",
                        esc(&id),
                        inst_type,
                        inst_family,
                        base_ccy,
                        quote_ccy,
                        settle_ccy,
                        list_time,
                        exp_time,
                        tick_sz,
                        lot_sz,
                        min_sz,
                        alias,
                        state,
                        inst_id_code,
                        inst_category
                    );
                    if let Err(e) = conn.execute(insert_sql.as_str(), []) {
                        log::error!("failed to insert instrument {}: {:?}", id, e);
                    } else {
                        existing.insert(id.clone());
                        if let Some(st) = info.state.clone() {
                            existing_state.insert(id.clone(), st);
                        }
                        new_ids.push(id.clone());
                    }
                } else {
                    // if exists, check state change — update whole row
                    let new_state = info.state.clone().unwrap_or_default();
                    let prev = existing_state.get(&id).cloned().unwrap_or_default();
                    if new_state != prev {
                        let update_sql = format!(
                            "UPDATE OKX_INSTRUMENTS SET instType='{}', instFamily={}, baseCcy='{}', quoteCcy={}, settleCcy={}, listTime={}, expTime={}, tickSz={}, lotSz={}, minSz={}, alias={}, state={}, instIdCode={}, instCategory={} WHERE instId='{}';",
                            inst_type,
                            inst_family,
                            base_ccy,
                            quote_ccy,
                            settle_ccy,
                            list_time,
                            exp_time,
                            tick_sz,
                            lot_sz,
                            min_sz,
                            alias,
                            state,
                            inst_id_code,
                            inst_category,
                            esc(&id)
                        );
                        if let Err(e) = conn.execute(update_sql.as_str(), []) {
                            log::error!("failed to update instrument {}: {:?}", id, e);
                        } else {
                            existing_state.insert(id.clone(), new_state.clone());
                        }
                    }
                }
            }
        }
        Err(e) => {
            log::error!("list_instruments error: {:?}", e);
        }
    }

    new_ids
}

///
/// 经过思考。最后决定，按照每个品类，进行处理。而不是统一的处理。
/// 1. 交易品种是会更新的。而交易品种的更新。这个也是需要维护的。
/// 2. 每个品种要维护的信息其实不一样。如果按照品类进行维护，那些纵向的排序很麻烦。
/// 3. 从用户角度，往往是交易几个具体的品种。而不是一起来弄的。
///
/// # 主要功能。
/// 1. 维护相应的inst_ids
/// 2. 维护Kline，包括Kline和option info
///
///
pub struct OptionService {
    pub inst_ids: Arc<RwLock<Vec<String>>>,
    pub api: OKxApi,
    pub sender: broadcast::Sender<OkxKlinePo>,
    pub duck_db_provider: DuckDBDSProvider,
    pub interval: HistoryInterval,
}

impl Default for OptionService {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl OptionService {
    pub fn new(duck_db_provider: Option<DuckDBDSProvider>, interval: Option<HistoryInterval>) -> Self {
        let (sender, _) = broadcast::channel(10000);
        Self {
            inst_ids: Arc::new(RwLock::new(vec![])),
            api: default_okx_api(),
            sender,
            duck_db_provider: duck_db_provider.unwrap_or_default(),
            interval: interval.unwrap_or(HistoryInterval::OneHour),
        }
    }

    pub async fn refresh_inst_ids(&self) {
        // call for BTC and ETH
        let _new_btc = fetch_and_upsert_instruments(InstrumentsParam::query_option("BTC-USD"), &self.duck_db_provider, &self.api).await;
        let _new_eth = fetch_and_upsert_instruments(InstrumentsParam::query_option("ETH-USD"), &self.duck_db_provider, &self.api).await;

        // acquire connection to collect live ids from DB
        let conn_res = self.duck_db_provider.acquire();
        let conn = match conn_res {
            Ok(c) => c,
            Err(e) => {
                log::error!("acquire connection error when refresh inst ids: {:?}", e);
                return;
            }
        };

        let mut live_ids: Vec<String> = Vec::new();
        if let Ok(mut stmt) = conn.prepare("SELECT instId FROM OKX_INSTRUMENTS WHERE state='live' and instType='OPTION';") {
            if let Ok(mut rows) = stmt.query([]) {
                loop {
                    match rows.next() {
                        Ok(Some(row)) => {
                            if let Ok(id) = row.get::<usize, String>(0) {
                                live_ids.push(id);
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            log::error!("error iterating live rows: {:?}", e);
                            break;
                        }
                    }
                }
            }
        }

        // deduplicate
        live_ids.sort();

        // update in-memory inst_ids
        match self.inst_ids.write() {
            Ok(mut guard) => {
                *guard = live_ids;
            }
            Err(e) => {
                log::error!("failed to acquire inst_ids write lock: {:?}", e);
            }
        }
    }

    pub fn initial_candle(&self) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::OptionService;
    use crate::okx::duckdb_tables::initial_okx_tables;
    use crate::test_utils::create_memory_db_provider;
    use async_trait::async_trait;
    use std::sync::Arc;
    use yue::errors::YueError;
    use yue::okx::models::common::{InstrumentInfo, OkxListResponse};
    use yue::okx::restful_api::OKXApiTrait;
    use yue::query_message::DataSourceProviderTrait;

    struct MockOkxApi;

    #[async_trait]
    impl OKXApiTrait for MockOkxApi {
        async fn list_instruments(&self, _params: yue::okx::restful_api::InstrumentsParam) -> Result<OkxListResponse<InstrumentInfo>, YueError> {
            // return both instruments, BTC live and ETH delist
            let btc = InstrumentInfo {
                alias: None,
                auction_end_time: None,
                base_ccy: "BTC".to_string(),
                category: None,
                ct_mult: None,
                ct_type: None,
                ct_val: None,
                ct_val_ccy: None,
                cont_td_sw_time: None,
                exp_time: None,
                future_settlement: None,
                group_id: None,
                inst_family: Some("BTC-USD".to_string()),
                inst_id: "BTC-OPT-1".to_string(),
                inst_type: "OPTION".to_string(),
                lever: None,
                list_time: None,
                lot_sz: None,
                max_iceberg_sz: None,
                max_lmt_amt: None,
                max_lmt_sz: None,
                max_mkt_amt: None,
                max_mkt_sz: None,
                max_stop_sz: None,
                max_trigger_sz: None,
                max_twap_sz: None,
                min_sz: None,
                opt_type: None,
                open_type: None,
                pre_mkt_sw_time: None,
                quote_ccy: None,
                trade_quote_ccy_list: None,
                settle_ccy: None,
                state: Some("live".to_string()),
                rule_type: None,
                stk: None,
                tick_sz: None,
                uly: None,
                inst_id_code: None,
                inst_category: None,
                upc_chg: None,
            };

            let eth = InstrumentInfo {
                alias: None,
                auction_end_time: None,
                base_ccy: "ETH".to_string(),
                category: None,
                ct_mult: None,
                ct_type: None,
                ct_val: None,
                ct_val_ccy: None,
                cont_td_sw_time: None,
                exp_time: None,
                future_settlement: None,
                group_id: None,
                inst_family: Some("ETH-USD".to_string()),
                inst_id: "ETH-OPT-1".to_string(),
                inst_type: "OPTION".to_string(),
                lever: None,
                list_time: None,
                lot_sz: None,
                max_iceberg_sz: None,
                max_lmt_amt: None,
                max_lmt_sz: None,
                max_mkt_amt: None,
                max_mkt_sz: None,
                max_stop_sz: None,
                max_trigger_sz: None,
                max_twap_sz: None,
                min_sz: None,
                opt_type: None,
                open_type: None,
                pre_mkt_sw_time: None,
                quote_ccy: None,
                trade_quote_ccy_list: None,
                settle_ccy: None,
                state: Some("delist".to_string()),
                rule_type: None,
                stk: None,
                tick_sz: None,
                uly: None,
                inst_id_code: None,
                inst_category: None,
                upc_chg: None,
            };

            let resp = OkxListResponse {
                code: "0".to_string(),
                msg: "ok".to_string(),
                data: vec![btc, eth],
            };
            Ok(resp)
        }

        async fn query_history_candle(
            &self,
            _params: yue::okx::restful_api::HistoryParams,
        ) -> Result<yue::okx::models::common::CandleResponse, YueError> {
            Err(YueError::new("not implemented"))
        }
    }

    #[tokio::test]
    async fn test_refresh_inst_ids_inserts_and_updates() {
        let provider = create_memory_db_provider();
        // create okx tables
        initial_okx_tables(Some(provider.clone())).expect("init tables");

        let mut svc = OptionService::new(Some(provider.clone()), None);
        // inject mock api
        svc.api = Arc::new(MockOkxApi {});

        // run refresh
        svc.refresh_inst_ids().await;

        // check db has inserted BTC-OPT-1 and columns populated
        let conn = provider.acquire().expect("acquire");
        let mut stmt = conn
            .prepare("SELECT instType, instFamily, baseCcy, state FROM OKX_INSTRUMENTS WHERE instId='BTC-OPT-1';")
            .unwrap();
        let row = stmt
            .query_row([], |row| {
                Ok((
                    row.get::<usize, String>(0)?,
                    row.get::<usize, String>(1)?,
                    row.get::<usize, String>(2)?,
                    row.get::<usize, String>(3)?,
                ))
            })
            .unwrap();
        assert_eq!(row.0, "OPTION".to_string());
        assert_eq!(row.1, "BTC-USD".to_string());
        assert_eq!(row.2, "BTC".to_string());
        assert_eq!(row.3, "live".to_string());

        // inst_ids should contain only live instruments (BTC-OPT-1)
        let guard = svc.inst_ids.read().unwrap();
        assert!(guard.contains(&"BTC-OPT-1".to_string()));
        assert!(!guard.contains(&"ETH-OPT-1".to_string()));
    }

    #[tokio::test]
    async fn test_refresh_inst_ids_updates_state_when_changed() {
        let provider = create_memory_db_provider();
        initial_okx_tables(Some(provider.clone())).expect("init tables");

        // pre-insert ETH with state 'delist'
        let pre_conn = provider.acquire().expect("acquire");
        pre_conn.execute("INSERT INTO OKX_INSTRUMENTS(instId, instType, instFamily, baseCcy, quoteCcy, settleCcy, listTime, expTime, tickSz, lotSz, minSz, alias, state, instIdCode, instCategory) VALUES ('ETH-OPT-1','OPTION','ETH-USD','ETH', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 'delist', NULL, NULL);", []).expect("insert eth");

        // create mock that returns ETH as live
        struct MockOkxApiLive;
        #[async_trait]
        impl OKXApiTrait for MockOkxApiLive {
            async fn list_instruments(&self, _params: yue::okx::restful_api::InstrumentsParam) -> Result<OkxListResponse<InstrumentInfo>, YueError> {
                let eth = InstrumentInfo {
                    alias: None,
                    auction_end_time: None,
                    base_ccy: "ETH".to_string(),
                    category: None,
                    ct_mult: None,
                    ct_type: None,
                    ct_val: None,
                    ct_val_ccy: None,
                    cont_td_sw_time: None,
                    exp_time: None,
                    future_settlement: None,
                    group_id: None,
                    inst_family: Some("ETH-USD".to_string()),
                    inst_id: "ETH-OPT-1".to_string(),
                    inst_type: "OPTION".to_string(),
                    lever: None,
                    list_time: None,
                    lot_sz: None,
                    max_iceberg_sz: None,
                    max_lmt_amt: None,
                    max_lmt_sz: None,
                    max_mkt_amt: None,
                    max_mkt_sz: None,
                    max_stop_sz: None,
                    max_trigger_sz: None,
                    max_twap_sz: None,
                    min_sz: None,
                    opt_type: None,
                    open_type: None,
                    pre_mkt_sw_time: None,
                    quote_ccy: None,
                    trade_quote_ccy_list: None,
                    settle_ccy: None,
                    state: Some("live".to_string()),
                    rule_type: None,
                    stk: None,
                    tick_sz: None,
                    uly: None,
                    inst_id_code: None,
                    inst_category: None,
                    upc_chg: None,
                };
                let resp = OkxListResponse {
                    code: "0".to_string(),
                    msg: "ok".to_string(),
                    data: vec![eth],
                };
                Ok(resp)
            }
            async fn query_history_candle(
                &self,
                _params: yue::okx::restful_api::HistoryParams,
            ) -> Result<yue::okx::models::common::CandleResponse, YueError> {
                Err(YueError::new("not implemented"))
            }
        }

        let mut svc = OptionService::new(Some(provider.clone()), None);
        svc.api = Arc::new(MockOkxApiLive {});
        svc.refresh_inst_ids().await;

        // verify state updated to live
        let conn = provider.acquire().expect("acquire");
        let state: String = conn
            .prepare("SELECT state FROM OKX_INSTRUMENTS WHERE instId='ETH-OPT-1';")
            .unwrap()
            .query_row([], |row| row.get(0))
            .unwrap();
        assert_eq!(state, "live".to_string());
    }
}
