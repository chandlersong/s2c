use crate::errors::YuError;
use crate::okx::duck_po::{InstrumentPo, OkxKlinePo};
use crate::okx::duckdb_repository::{OkxInstrumentRepository, OkxKlineRepository, get_instrument_repo};
use crate::okx::okx_consts::InstrumentType;
use governor::Jitter;
use log::{error, warn};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::broadcast;
use yue::models::HistoryInterval;
use yue::okx::restful_api::{HistoryParams, InstrumentsParam, OKxApi, default_okx_api};

///
/// # 大致流程
///
/// 1. 从start_ts到end_ts之间。调用query_history_candle
/// 2. 每次有新的值，找到返回值中，最小的ts。然后更新current_end_ts
/// 3. 满足退出条件。返回。
///
/// ### 查询退出条件
/// 1. api返回的kline最小的的timestamp小于start_ts。
/// 2. api返回的kline为空
/// 3. 返回的条数，小于limit_num
///
/// ### 过滤条件
/// 满足以下条件，被删除出返回值。
/// 1. 返回的数据的ts。不在start_ts和end_ts
/// 2. 返回confirm为0
///
/// # 业务条件。
/// 1. 每一次查询的kline，都通过kline_repository的batch_insert存入数据库。
/// 2. 有些kline可能为空
/// 3. OkxApi返回的kline的timestamp是降序的。
///
///
///
pub async fn fetch_history(
    inst_id: &str,
    start_ts: u64,
    end_ts: u64,
    interval: &HistoryInterval,
    api: &OKxApi,
    kline_repository: &OkxKlineRepository,
    limit_num: Option<u64>,
    max_error_num: Option<usize>,
) -> Result<Vec<OkxKlinePo>, YuError> {
    let mut all_records: Vec<OkxKlinePo> = Vec::new();

    let mut error_count: usize = 0;
    let mut current_end_ts = end_ts;
    let inst_id_up = inst_id.to_uppercase();
    let limit = limit_num.unwrap_or(300).to_string();
    let jitter = Jitter::up_to(Duration::from_millis(2000));
    loop {
        // build params: omit `before` for the very first call (no pagination cursor)
        let request_param = HistoryParams::builder()
            .inst_id(inst_id.to_string())
            .after(current_end_ts.to_string())
            .before(start_ts.to_string())
            .bar(interval.as_ref().to_uppercase())
            .limit(limit.clone())
            .build();

        let candles = match api.query_history_candle(request_param).await {
            Ok(candle) => candle,
            Err(e) => {
                error!("Error fetching candles: {}", e);
                tokio::time::sleep(jitter + Duration::from_secs(10)).await; // backoff before retrying
                error_count = error_count.saturating_add(1);
                if max_error_num.map_or(false, |max| error_count > max) {
                    return Err(YuError::MaxErrorReached("fetch okx kline net error".to_string(), error_count));
                }
                continue;
            }
        };

        let mut batch = OkxKlinePo::from_kline_response(&inst_id_up, candles);

        // filter out records outside [start_ts, end_ts] or confirm == 0
        batch.retain(|p| p.ts >= start_ts && p.ts <= end_ts && p.confirm != 0);

        if batch.is_empty() {
            // no relevant data
            break;
        }

        // find minimal timestamp in this batch
        let min_ts_opt = batch.iter().map(|p| p.ts).min();

        // insert into db (clone since we'll append afterwards)
        let insert_batch = batch.clone();
        if let Err(e) = kline_repository.batch_insert(insert_batch).await {
            error!("Error inserting batch into database: {}", e);
            tokio::time::sleep(jitter + Duration::ZERO).await; // wait a bit before retrying
            error_count = error_count.saturating_add(1);
            if max_error_num.map_or(false, |max| error_count > max) {
                return Err(YuError::MaxErrorReached("fetch okx kline net error".to_string(), error_count));
            }
            continue;
        }

        // append to result
        all_records.append(&mut batch);

        if let Some(min_ts) = min_ts_opt {
            if min_ts <= start_ts {
                break;
            } else if min_ts > 0 {
                // set current_end_ts to one less than min_ts to paginate
                current_end_ts = min_ts - 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    Ok(all_records)
}

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
pub async fn fetch_and_upsert_instruments(
    param: InstrumentsParam,
    instrument_repo: &OkxInstrumentRepository,
    api: &OKxApi,
    inst_type: InstrumentType,
) -> Result<(), YuError> {
    let mut existing: HashMap<String, InstrumentPo> = HashMap::new();
    let instruments = instrument_repo.get_instrument_by_type(inst_type).await?;
    for instr in instruments {
        existing.insert(instr.inst_id.clone(), instr);
    }
    // call API
    match api.list_instruments(param).await {
        Ok(resp) => {
            for info in resp.data.into_iter() {
                let id = info.inst_id.clone();
                if !existing.contains_key(&id) {
                    // insert full row
                    let po = InstrumentPo::from(info);
                    existing.insert(id.clone(), po.clone());
                    if let Err(e) = instrument_repo.insert_instrument(po).await {
                        warn!("Failed to insert instrument: {:?}", e);
                    }
                } else {
                    let mut po = None;
                    let db_state = existing.get(&id).unwrap().state.clone();
                    if db_state == None {
                        po = Some(InstrumentPo::from(info));
                    } else {
                        let api_state = info.state.clone();
                        if api_state != db_state {
                            po = Some(InstrumentPo::from(info));
                        }
                    }

                    if let Some(inst_po) = po {
                        if let Err(e) = instrument_repo.update_instrument(inst_po).await {
                            warn!("Failed to update instrument: {:?}", e);
                        }
                    }
                }
            }
        }
        Err(e) => {
            log::error!("list_instruments error: {:?}", e);
        }
    };
    Ok(())
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
    pub inst_ids: Arc<RwLock<Vec<InstrumentPo>>>,
    pub api: OKxApi,
    pub sender: broadcast::Sender<OkxKlinePo>,
    pub instrument_repo: OkxInstrumentRepository,
    pub interval: HistoryInterval,
}

impl Default for OptionService {
    fn default() -> Self {
        Self::new(None, None, None)
    }
}

impl OptionService {
    pub fn new(instrument_repo: Option<OkxInstrumentRepository>, api: Option<OKxApi>, interval: Option<HistoryInterval>) -> Self {
        let (sender, _) = broadcast::channel(10000);
        Self {
            inst_ids: Arc::new(RwLock::new(vec![])),
            api: api.unwrap_or_else(|| default_okx_api()),
            sender,
            instrument_repo: instrument_repo.unwrap_or_else(|| get_instrument_repo(None)),
            interval: interval.unwrap_or(HistoryInterval::OneHour),
        }
    }

    pub async fn refresh_inst_ids(&self) {
        // call for BTC and ETH
        let _ = fetch_and_upsert_instruments(
            InstrumentsParam::query_option("BTC-USD"),
            &self.instrument_repo,
            &self.api,
            InstrumentType::Option,
        )
        .await;
        let _ = fetch_and_upsert_instruments(
            InstrumentsParam::query_option("ETH-USD"),
            &self.instrument_repo,
            &self.api,
            InstrumentType::Option,
        )
        .await;
        let live_instruments = self.instrument_repo.get_instrument_by_type_live(InstrumentType::Option).await;
        match live_instruments {
            Ok(instruments) => {
                // update in-memory inst_ids
                match self.inst_ids.write() {
                    Ok(mut guard) => {
                        *guard = instruments;
                    }
                    Err(e) => {
                        log::error!("failed to acquire inst_ids write lock: {:?}", e);
                    }
                }
            }
            Err(err) => {
                log::error!("skip refresh failed to query okx instruments: {:?}", err);
            }
        }
    }

    pub fn initial_candle(&self) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::{fetch_and_upsert_instruments, fetch_history};
    use crate::errors::YuError;
    use crate::okx::duck_po::InstrumentPo;
    use crate::okx::duckdb_repository::OkxInstrumentRepository;
    use crate::okx::duckdb_repository::{MockOkxInstrumentRepositoryTrait, MockOkxKlineRepositoryTrait, OkxKlineRepository};
    use crate::okx::duckdb_tables::initial_okx_tables;
    use crate::okx::okx_consts::InstrumentType;
    use crate::test_utils::create_memory_db_provider;
    use std::sync::Arc;
    use yue::models::HistoryInterval;
    use yue::okx::models::common::{CandleResponse, InstrumentInfo, OkxListResponse};
    use yue::okx::restful_api::{InstrumentsParam, MockOKXApiTrait, OKxApi};

    ///
    /// 检测如果api返回的数据，不存在数据库中。则会插入数据库
    ///  1. OKx Api返回连个BTC_1
    ///  2. 数据库中，没有数据
    ///  判断：
    ///    BTC_1加入数据库
    ///
    #[tokio::test]
    async fn test_fetch_and_upsert_instruments_new() {
        let param = InstrumentsParam::query_option("BTC-USD");
        let provider = create_memory_db_provider();
        // create okx tables
        initial_okx_tables(Some(provider.clone())).expect("init tables");

        let mut mock_api = MockOKXApiTrait::new();
        let btc = InstrumentInfo::builder()
            .inst_id("BTC-1".to_string())
            .state("live".to_string())
            .inst_type("OPTION".to_string())
            .base_ccy("BTC".to_string())
            .build();
        mock_api.expect_list_instruments().return_once(|_| {
            Ok(OkxListResponse {
                code: "0".to_string(),
                msg: "success".to_string(),
                data: vec![btc],
            })
        });

        let mut mock_inst_repo = MockOkxInstrumentRepositoryTrait::new();
        mock_inst_repo.expect_get_instrument_by_type().return_once(|_| Ok(vec![]));

        mock_inst_repo
            .expect_insert_instrument()
            .times(1)
            .withf(|instrument_info| instrument_info.inst_id == "BTC-1")
            .returning(|_| Ok(()));

        let inst_repo: OkxInstrumentRepository = Arc::new(mock_inst_repo);
        let api: OKxApi = Arc::new(mock_api);
        fetch_and_upsert_instruments(param, &inst_repo, &api, InstrumentType::Option)
            .await
            .expect("TODO: panic message");
    }

    ///
    /// 检测如果api返回的数据，存在数据库中，则会更新
    ///
    /// 1. OKx Api返回连个BTC_1和BTC_2,state都是live
    /// 2. 数据库中，有BTC_1, state是live，BTC_2存在，state为suspend
    ///
    /// 判断：
    /// BTC_1不更新，BTC_2会更新为live
    ///
    #[tokio::test]
    async fn test_refresh_inst_ids_updates_state_changes() {
        let param = InstrumentsParam::query_option("BTC-USD");
        let provider = create_memory_db_provider();
        initial_okx_tables(Some(provider.clone())).expect("init tables");

        // API returns BTC-1 (live) and BTC-2 (live)
        let mut mock_api = MockOKXApiTrait::new();
        let btc1 = InstrumentInfo::builder()
            .inst_id("BTC-1".to_string())
            .state("live".to_string())
            .inst_type("OPTION".to_string())
            .base_ccy("BTC".to_string())
            .build();
        let btc2 = InstrumentInfo::builder()
            .inst_id("BTC-2".to_string())
            .state("live".to_string())
            .inst_type("OPTION".to_string())
            .base_ccy("BTC".to_string())
            .build();
        mock_api.expect_list_instruments().return_once(move |_| {
            Ok(OkxListResponse {
                code: "0".to_string(),
                msg: "success".to_string(),
                data: vec![btc1.clone(), btc2.clone()],
            })
        });

        // DB has BTC-1 with state live, BTC-2 with state suspend
        let mut mock_inst_repo = MockOkxInstrumentRepositoryTrait::new();
        let db_btc1 = InstrumentPo::builder()
            .inst_id("BTC-1".to_string())
            .inst_type("OPTION".to_string())
            .base_ccy("BTC".to_string())
            .state("live".to_string())
            .build();
        let db_btc2 = InstrumentPo::builder()
            .inst_id("BTC-2".to_string())
            .inst_type("OPTION".to_string())
            .base_ccy("BTC".to_string())
            .state("suspend".to_string())
            .build();

        mock_inst_repo
            .expect_get_instrument_by_type()
            .return_once(move |_| Ok(vec![db_btc1, db_btc2]));

        // expect update_instrument called for BTC-2 (state changed from suspend -> live)
        mock_inst_repo
            .expect_update_instrument()
            .times(1)
            .withf(|instrument_info: &InstrumentPo| instrument_info.inst_id == "BTC-2" && instrument_info.state == Some("live".to_string()))
            .returning(|_| Ok(()));

        let inst_repo: OkxInstrumentRepository = Arc::new(mock_inst_repo);
        let api: OKxApi = Arc::new(mock_api);

        fetch_and_upsert_instruments(param, &inst_repo, &api, InstrumentType::Option)
            .await
            .expect("fetch failed");
    }

    ///
    /// 测试数据库，完全没有的数据情况下，存入数据库。
    /// inst_id为BTC-1为例子。
    /// 1. 所有的数据，满足正常要求。没有被过滤
    ///
    #[tokio::test]
    pub async fn test_fetch_history_new() -> Result<(), YuError> {
        let interval = HistoryInterval::OneHour;
        let start = interval.to_milliseconds();
        let end = start + 3 * interval.to_milliseconds() + 1;
        let mut mock_kline_repo = MockOkxKlineRepositoryTrait::new();
        mock_kline_repo.expect_instrument_max_timestamp().return_once(|_| Ok(None));
        mock_kline_repo.expect_batch_insert().times(2).returning(|_| Ok(()));

        let mut mock_api = MockOKXApiTrait::new();
        let response_1: CandleResponse = OkxListResponse {
            code: "0".to_string(),
            msg: "".to_string(),
            data: vec![
                vec![
                    (start + interval.to_milliseconds() * 2).to_string(),
                    "3.721".to_string(),
                    "3.743".to_string(),
                    "3.677".to_string(),
                    "3.708".to_string(),
                    "8422410".to_string(),
                    "22698348.04828491".to_string(),
                    "12698348.04828491".to_string(),
                    "1".to_string(),
                ],
                vec![
                    (start + interval.to_milliseconds()).to_string(),
                    "3.731".to_string(),
                    "3.799".to_string(),
                    "3.494".to_string(),
                    "3.72".to_string(),
                    "24912403".to_string(),
                    "67632347.24399722".to_string(),
                    "37632347.24399722".to_string(),
                    "1".to_string(),
                ],
            ],
        };
        let response_2: CandleResponse = OkxListResponse {
            code: "0".to_string(),
            msg: "".to_string(),
            data: vec![vec![
                start.to_string(),
                "3.731".to_string(),
                "3.799".to_string(),
                "3.494".to_string(),
                "3.72".to_string(),
                "24912403".to_string(),
                "67632347.24399722".to_string(),
                "37632347.24399722".to_string(),
                "1".to_string(),
            ]],
        };
        let during_ms = interval.to_milliseconds();
        mock_api
            .expect_query_history_candle()
            .withf(move |param| param.after == Some(end.to_string()))
            .return_once(move |_| Ok(response_1));
        mock_api
            .expect_query_history_candle()
            .withf(move |param| param.after == Some((start + during_ms - 1).to_string()))
            .return_once(move |_| Ok(response_2));

        let api: OKxApi = Arc::new(mock_api);
        let kline_repo: OkxKlineRepository = Arc::new(mock_kline_repo);

        let res = fetch_history("btc-usd", start, end, &interval, &api, &kline_repo, Some(2), None).await?;
        assert_eq!(res.len(), 3);

        Ok(())
    }

    ///
    /// 有一条数据为没有完成，就是confirm为0
    ///
    /// 应该过滤
    ///
    ///
    #[tokio::test]
    pub async fn test_fetch_history_with_incomplete_kline() -> Result<(), YuError> {
        let interval = HistoryInterval::OneHour;
        let start = interval.to_milliseconds();
        let end = start + 3 * interval.to_milliseconds() + 1;
        let mut mock_kline_repo = MockOkxKlineRepositoryTrait::new();
        mock_kline_repo.expect_instrument_max_timestamp().return_once(|_| Ok(None));
        mock_kline_repo.expect_batch_insert().times(2).returning(|_| Ok(()));

        let mut mock_api = MockOKXApiTrait::new();
        let response_1: CandleResponse = OkxListResponse {
            code: "0".to_string(),
            msg: "".to_string(),
            data: vec![
                vec![
                    (start + interval.to_milliseconds() * 2).to_string(),
                    "3.721".to_string(),
                    "3.743".to_string(),
                    "3.677".to_string(),
                    "3.708".to_string(),
                    "8422410".to_string(),
                    "22698348.04828491".to_string(),
                    "12698348.04828491".to_string(),
                    "0".to_string(),
                ],
                vec![
                    (start + interval.to_milliseconds()).to_string(),
                    "3.731".to_string(),
                    "3.799".to_string(),
                    "3.494".to_string(),
                    "3.72".to_string(),
                    "24912403".to_string(),
                    "67632347.24399722".to_string(),
                    "37632347.24399722".to_string(),
                    "1".to_string(),
                ],
            ],
        };
        let response_2: CandleResponse = OkxListResponse {
            code: "0".to_string(),
            msg: "".to_string(),
            data: vec![vec![
                start.to_string(),
                "3.731".to_string(),
                "3.799".to_string(),
                "3.494".to_string(),
                "3.72".to_string(),
                "24912403".to_string(),
                "67632347.24399722".to_string(),
                "37632347.24399722".to_string(),
                "1".to_string(),
            ]],
        };
        let during_ms = interval.to_milliseconds();
        mock_api
            .expect_query_history_candle()
            .withf(move |param| param.after == Some(end.to_string()))
            .return_once(move |_| Ok(response_1));
        mock_api
            .expect_query_history_candle()
            .withf(move |param| param.after == Some((start + during_ms - 1).to_string()))
            .return_once(move |_| Ok(response_2));

        let api: OKxApi = Arc::new(mock_api);
        let kline_repo: OkxKlineRepository = Arc::new(mock_kline_repo);

        let res = fetch_history("btc-usd", start, end, &interval, &api, &kline_repo, Some(2), None).await?;
        assert_eq!(res.len(), 2);

        Ok(())
    }

    ///
    /// 因为太慢了。所以手工跑吧。
    ///
    #[ignore]
    #[tokio::test]
    pub async fn test_fetch_history_max_error_reached() {
        let interval = HistoryInterval::OneHour;
        let start = interval.to_milliseconds();
        let end = start + interval.to_milliseconds();

        let mut mock_kline_repo = MockOkxKlineRepositoryTrait::new();
        mock_kline_repo.expect_instrument_max_timestamp().return_once(|_| Ok(None));

        let mut mock_api = MockOKXApiTrait::new();
        // Simulate repeated network errors: fetch_history should return MaxErrorReached after exceeding max_error_num
        mock_api
            .expect_query_history_candle()
            .times(3)
            .returning(|_| Err(yue::errors::YueError::new("network error")));

        let api: OKxApi = Arc::new(mock_api);
        let kline_repo: OkxKlineRepository = Arc::new(mock_kline_repo);

        let res = fetch_history("btc-usd", start, end, &interval, &api, &kline_repo, Some(2), Some(2)).await;
        match res {
            Err(YuError::MaxErrorReached(msg, cnt)) => {
                assert!(msg.contains("fetch okx kline net error"));
                // first two errors are retried, third should trigger MaxErrorReached -> cnt == 3
                assert_eq!(cnt, 3);
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }
}
