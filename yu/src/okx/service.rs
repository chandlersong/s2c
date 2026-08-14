use crate::config::get_config;
use crate::cron_job;
use crate::errors::YuError;
use crate::okx::duck_po::{InstrumentPo, OkxKlinePo};
use crate::okx::duckdb_repository::{OkxInstrumentRepository, OkxKlineRepository, get_default_kline_repo, get_instrument_repo};
use crate::okx::okx_consts::InstrumentType;
use async_trait::async_trait;
use futures::stream::StreamExt;
use governor::Jitter;
use li::tools::time::{UnixTimeStamp, unix_2_readable};
use li::websocket::connection::{
    CommandMessage, ConnectionAction, ConnectionConfig, MessageHandlerTrait, ShareMessageHandler, ToServerMessage, WebSocketConnection,
    WebSocketInterface,
};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use yue::models::HistoryInterval;
use yue::okx::models::websocket::{ArgBody, OkxWebsocketResponse};
use yue::okx::restful_api::{HistoryParams, InstrumentsParam, OKxApi, default_okx_api};
use yue::okx::websocket_channel::{CommandRequest, OXK_BUSINESS_WEBSOCKET};

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait::async_trait]
pub trait CommonIOServiceTrait {
    fn get_kline_repo(&self) -> OkxKlineRepository;
    fn get_instrument_repo(&self) -> OkxInstrumentRepository;
    fn get_okx_api(&self) -> OKxApi;
    async fn fetch_history(
        &self,
        inst_identify: &str,
        inst_id: u64,
        start_ts: u64,
        end_ts: u64,
        interval: &HistoryInterval,
        limit_num: Option<u64>,
        max_error_num: Option<usize>,
    ) -> Result<Vec<OkxKlinePo>, YuError>;
    async fn fetch_and_update_instruments(&self, param: InstrumentsParam, inst_type: InstrumentType) -> Result<(), YuError>;
}

pub type CommonIOService = Arc<dyn CommonIOServiceTrait + Send + Sync>;

fn create_common_io_service(instrument_repo: OkxInstrumentRepository, kline_repo: OkxKlineRepository, okx_api: OKxApi) -> CommonIOService {
    Arc::new(CommonIOServiceImpl::new(instrument_repo, kline_repo, okx_api))
}
///
/// 因为按照OKX的数据结构。所有的交易标的都是instrument的结构。
/// 然后历史等信息，基本一致。所以把这类方法抽象到这里，方便日后的重写。
///
/// 而这个方法的主要目的，还是为了测试.方便mock，而方法又不能mock。所以用了这种方法。
/// 而套一层，存粹是因为没办法做其他的。
///
struct CommonIOServiceImpl {
    instrument_repo: OkxInstrumentRepository,
    kline_repo: OkxKlineRepository,
    okx_api: OKxApi,
}

impl Default for CommonIOServiceImpl {
    fn default() -> Self {
        Self {
            instrument_repo: get_instrument_repo(None),
            kline_repo: get_default_kline_repo(None),
            okx_api: default_okx_api(),
        }
    }
}

impl CommonIOServiceImpl {
    pub fn new(instrument_repo: OkxInstrumentRepository, kline_repo: OkxKlineRepository, okx_api: OKxApi) -> Self {
        Self {
            instrument_repo,
            kline_repo,
            okx_api,
        }
    }
}
#[async_trait::async_trait]
impl CommonIOServiceTrait for CommonIOServiceImpl {
    fn get_kline_repo(&self) -> OkxKlineRepository {
        self.kline_repo.clone()
    }

    fn get_instrument_repo(&self) -> OkxInstrumentRepository {
        self.instrument_repo.clone()
    }

    fn get_okx_api(&self) -> OKxApi {
        self.okx_api.clone()
    }

    async fn fetch_history(
        &self,
        inst_identify: &str,
        inst_id: u64,
        start_ts: u64,
        end_ts: u64,
        interval: &HistoryInterval,
        limit_num: Option<u64>,
        max_error_num: Option<usize>,
    ) -> Result<Vec<OkxKlinePo>, YuError> {
        fetch_history(
            inst_identify,
            inst_id,
            start_ts,
            end_ts,
            interval,
            &self.okx_api,
            &self.kline_repo,
            limit_num,
            max_error_num,
        )
        .await
    }

    async fn fetch_and_update_instruments(&self, param: InstrumentsParam, inst_type: InstrumentType) -> Result<(), YuError> {
        fetch_and_update_instruments(param, inst_type, &self.instrument_repo, &self.okx_api).await
    }
}
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
    inst_identify: &str,
    inst_id: u64,
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
    let inst_id_up = inst_identify;
    let limit = limit_num.unwrap_or(300).to_string();
    let jitter = Jitter::up_to(Duration::from_millis(2000));
    loop {
        // build params: omit `before` for the very first call (no pagination cursor)
        let request_param = HistoryParams::builder()
            .inst_id(inst_id_up.to_string())
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

        let mut batch = OkxKlinePo::from_kline_response(inst_id, candles);

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
pub async fn fetch_and_update_instruments(
    param: InstrumentsParam,
    inst_type: InstrumentType,
    instrument_repo: &OkxInstrumentRepository,
    api: &OKxApi,
) -> Result<(), YuError> {
    let mut existing: HashMap<String, InstrumentPo> = HashMap::new();
    let instruments = instrument_repo.get_instrument_by_type(inst_type).await?;
    for instr in instruments {
        existing.insert(instr.inst_identify.clone(), instr);
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

pub struct KlineHandler {
    kline_repo: OkxKlineRepository,
    instrument_repo: OkxInstrumentRepository,
    inst_id_identify_map: Arc<RwLock<HashMap<String, InstrumentPo>>>,
}

impl KlineHandler {
    pub fn new(
        kline_repo: OkxKlineRepository,
        instrument_repo: OkxInstrumentRepository,
        inst_id_identify_map: HashMap<String, InstrumentPo>,
    ) -> Self {
        Self {
            kline_repo,
            instrument_repo,
            inst_id_identify_map: Arc::new(RwLock::new(inst_id_identify_map)),
        }
    }
}

#[async_trait]
impl MessageHandlerTrait<OkxWebsocketResponse> for KlineHandler {
    ///
    /// 处理订阅的消息。对于订阅的消息。
    /// 1. 把Kline的数据存入数据库。
    /// 2. 判断已经订阅的数据
    ///
    async fn handle_message(&self, message: &OkxWebsocketResponse) {
        match message {
            OkxWebsocketResponse::SubscribeResponse(_) => {
                //FUTURE: 这里可以判断一下是订阅成功
                //看到okx是一个一个回来的，比如说我订阅了A，B，C。一起订阅的，但是最后回来的是三条消息。
            }
            OkxWebsocketResponse::Kline(payload) => {
                // 如果查不到，就不如数据库，主要是为了防止脏数据。然后数据应该在更新instrument的时候补全。
                let inst_identify = payload.arg.inst_id.to_string();
                // 尝试从内存map读取
                let mut inst_opt = {
                    let guard = self.inst_id_identify_map.read().unwrap();
                    guard.get(&inst_identify).cloned()
                };

                // 如果内存中没有，从仓库查询并更新map
                if inst_opt.is_none() {
                    match self.instrument_repo.get_instrument_by_identify(&inst_identify).await {
                        Ok(po) => {
                            match self.inst_id_identify_map.write() {
                                Ok(mut w) => {
                                    w.insert(inst_identify.clone(), po.clone());
                                }
                                Err(e) => {
                                    warn!("failed to acquire inst_id_identify_map write lock: {:?}", e);
                                }
                            }
                            inst_opt = Some(po);
                        }
                        Err(e) => {
                            warn!("Received kline for unknown instrument: {}: {:?}", inst_identify, e);
                            return;
                        }
                    }
                }

                let inst = match inst_opt {
                    Some(i) => i,
                    None => {
                        // 理论上不应到这里，但防御性返回
                        warn!("Instrument lookup failed for {}", inst_identify);
                        return;
                    }
                };

                let pos = OkxKlinePo::from_ws_response(inst.id, &payload);
                for po in pos {
                    if po.confirm != 1 {
                        //过滤没有完成的kline
                        continue;
                    }
                    if let Err(e) = self.kline_repo.insert_history(po).await {
                        warn!("Failed to insert okx kline history: {:?}", e);
                    }
                }
            }
        }
    }
}

///
/// 经过思考。最后决定，按照每个品类，进行处理。而不是统一的处理。
/// 1. 交易品种是会更新的。而交易品种的更新。这个也是需要维护的。
/// 2. 每个品种要维护的信息其实不一样。如果按照品类进行维护，那些纵向的排序很麻烦。
/// 3. 从用户角度，往往是交易几个具体的品种。而不是一起来弄的。
///
/// # 主要的功能
/// 1. 更新instrument和更新相应的订阅kline的数据。
///     - 这个其实是该服务驱动的第一步。
/// 2. 订阅合理的Kline数据。然后发送
/// 3. 检查数据差异。如果必要初始化数据
///
///
///
pub struct OptionService {
    instruments: Arc<RwLock<Vec<InstrumentPo>>>,
    common_io: CommonIOService,
    interval: HistoryInterval,
    refresh_corn: String,
}

impl Default for OptionService {
    fn default() -> Self {
        Self::new(None, None, None, None, None)
    }
}

impl OptionService {
    pub fn new(
        instrument_repo: Option<OkxInstrumentRepository>,
        kline_repo: Option<OkxKlineRepository>,
        api: Option<OKxApi>,
        interval: Option<HistoryInterval>,
        refresh_corn: Option<String>,
    ) -> Self {
        Self {
            instruments: Arc::new(RwLock::new(vec![])),
            common_io: create_common_io_service(
                instrument_repo.unwrap_or_else(|| get_instrument_repo(None)),
                kline_repo.unwrap_or_else(|| get_default_kline_repo(None)),
                api.unwrap_or_else(|| default_okx_api()),
            ),
            interval: interval.unwrap_or(HistoryInterval::OneHour),
            refresh_corn: refresh_corn.unwrap_or("18 18 */6 * * *".to_string()),
        }
    }

    #[cfg(test)]
    fn new_with_mock(inst_ids: Arc<RwLock<Vec<InstrumentPo>>>, common_io: CommonIOService, interval: HistoryInterval) -> Self {
        Self {
            instruments: inst_ids,
            common_io,
            interval,
            refresh_corn: "18 * * * * *".to_string(),
        }
    }

    pub fn update_instruments(share_instruments: Arc<RwLock<Vec<InstrumentPo>>>, instruments: Vec<InstrumentPo>) -> Result<(), YuError> {
        match share_instruments.write() {
            Ok(mut guard) => {
                *guard = instruments;
                Ok(())
            }
            Err(e) => {
                log::error!("failed to acquire instruments write lock: {:?}", e);
                Err(YuError::CustomError(format!("failed to acquire instruments write lock: {:?}", e)))
            }
        }
    }

    pub async fn initial_instruments(&self) -> Result<Vec<InstrumentPo>, YuError> {
        let instruments = Self::refresh_instruments(self.common_io.clone(), self.instruments.clone()).await?;
        Ok(instruments)
    }

    ///
    /// 开启一个定时任务。定时任务的主要功能是刷新inst_ids。然后更新订阅的kline。
    ///
    pub async fn start(&self) -> Result<broadcast::Sender<OkxKlinePo>, YuError> {
        let instruments_share = self.instruments.clone();
        let common_io = self.common_io.clone();
        let instruments = Self::refresh_instruments(common_io.clone(), instruments_share.clone()).await?;
        let kline_repo = self.common_io.get_kline_repo();
        let instrument_repo = self.common_io.get_instrument_repo();
        let id_identify_dict = instruments
            .clone()
            .into_iter()
            .map(|inst| (inst.inst_identify.clone(), inst))
            .collect::<HashMap<String, InstrumentPo>>();
        let handler = Arc::new(KlineHandler::new(kline_repo, instrument_repo, id_identify_dict));
        let app_config = get_config();
        let proxy = app_config.proxy_url.clone();
        let interval = self.interval.clone();
        let websocket_interface = Self::listen_option_kline(instruments, &interval, proxy.clone(), Some(handler.clone()), None).await?;
        let (tx, _) = broadcast::channel(1000);
        // shared interface stored across cron_job invocations
        let shared_interface: Arc<Mutex<Arc<WebSocketInterface<OkxWebsocketResponse>>>> = Arc::new(Mutex::new(websocket_interface));

        let interface_for_cron = shared_interface.clone();
        let instruments_for_cron = instruments_share.clone();
        let handler_for_cron = handler.clone();
        let _ = cron_job!(self.refresh_corn.as_str(), move |_uuid, _locked| {
            let instruments_each = instruments_for_cron.clone();
            let common_io_each = common_io.clone();
            let interface_each = interface_for_cron.clone();
            let proxy_each = proxy.clone();
            let handler_each = handler_for_cron.clone();
            let interval_each = interval.clone();
            Box::pin(async move {
                info!("开始刷新okx option");
                let _ = Self::refresh_instruments(common_io_each.clone(), instruments_each.clone()).await;
                info!("结束刷新okx option");

                // after refreshing instruments, exercise websocket: recreate and swap
                let insts = match instruments_each.read() {
                    Ok(g) => g.clone(),
                    Err(e) => {
                        error!("failed to read instruments for ws exercise in cron: {:?}", e);
                        return;
                    }
                };

                if insts.is_empty() {
                    return;
                };

                match Self::listen_option_kline(insts.clone(), &interval_each, proxy_each, Some(handler_each), None).await {
                    Ok(new_interface) => {
                        // swap old interface with new one, then try close old
                        let mut guard = interface_each.lock().await;
                        // replace the current Arc with the new one, taking ownership of the old Arc
                        let old_interface = std::mem::replace(&mut *guard, new_interface.clone());

                        // attempt to close previous connection
                        let cmd_sender = old_interface.command_sender();
                        let mut attempt = 0usize;
                        loop {
                            attempt += 1;
                            let cmd = CommandMessage::Connection(ConnectionAction::Close);
                            match cmd_sender.send(cmd) {
                                Ok(_) => {
                                    info!("Sent Close command to previous connection (attempt {})", attempt);
                                    break;
                                }
                                Err(e) => {
                                    error!("Error close prev connection on attempt {}: {:?}", attempt, e);
                                    if attempt >= 10 {
                                        error!("Giving up closing previous connection after {} attempts.", attempt);
                                        break;
                                    } else {
                                        let jitter = Jitter::up_to(Duration::from_millis(500));
                                        tokio::time::sleep(jitter + Duration::ZERO).await;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("failed to recreate websocket for exercise in cron: {:?}", e);
                    }
                }
            })
        });

        Ok(tx)
    }

    ///
    /// 根据batch_num,分批订阅kline，为了防止超过64K的限制
    ///
    pub async fn listen_option_kline(
        instruments: Vec<InstrumentPo>,
        interval: &HistoryInterval,
        proxy: Option<String>,
        handler: Option<ShareMessageHandler<OkxWebsocketResponse>>,
        batch_num: Option<usize>,
    ) -> Result<Arc<WebSocketInterface<OkxWebsocketResponse>>, YuError> {
        let websocket_config = ConnectionConfig::builder()
            .ping_frequency(Duration::from_secs(28))
            .reconnect_interval(Duration::from_secs(5))
            .build();
        let interface =
            WebSocketConnection::run::<OkxWebsocketResponse>(OXK_BUSINESS_WEBSOCKET.to_string(), Some(websocket_config), proxy, handler).await;
        info!("✓ oxk kline WebSocket 客户端已启动");
        let batch_num = batch_num.unwrap_or(380);
        let frequency = match interval {
            HistoryInterval::OneMinute => "candle1m".to_string(),
            HistoryInterval::FiveMinutes => "candle5m".to_string(),
            HistoryInterval::OneHour => "candle1h".to_string(),
        };
        for (i, chunk) in instruments.chunks(batch_num).enumerate() {
            let mut args = vec![];
            for inst in chunk {
                let arg = ArgBody::builder()
                    .channel(frequency.clone())
                    .inst_id(inst.inst_identify.to_string())
                    .build();
                args.push(arg);
            }

            let request = CommandRequest::builder()
                .id((i + 1).to_string())
                .op("subscribe".to_string())
                .args(args)
                .build();
            let command_test = serde_json::to_string(&request)?;
            interface.send_command(CommandMessage::ToServer(ToServerMessage::text(command_test)));
        }

        Ok(interface)
    }

    pub async fn list_instruments(&self) -> Result<Vec<InstrumentPo>, YuError> {
        let live_instruments = self
            .common_io
            .get_instrument_repo()
            .get_instrument_by_type_live(InstrumentType::Option)
            .await;
        match live_instruments {
            Ok(instruments) => Ok(instruments),
            Err(err) => {
                log::error!("failed to query okx instruments: {:?}", err);
                Err(err)
            }
        }
    }

    pub async fn find_candle_after(&self, inst_id: u64, ts: u64) -> Result<Vec<OkxKlinePo>, YuError> {
        let kline_repo = self.common_io.get_kline_repo();
        kline_repo.find_kline_after(inst_id, ts).await
    }

    pub async fn refresh_instruments(
        common_io: CommonIOService,
        share_instruments: Arc<RwLock<Vec<InstrumentPo>>>,
    ) -> Result<Vec<InstrumentPo>, YuError> {
        // call for BTC and ETH
        let _ = common_io
            .fetch_and_update_instruments(InstrumentsParam::query_option("BTC-USD"), InstrumentType::Option)
            .await;
        let _ = common_io
            .fetch_and_update_instruments(InstrumentsParam::query_option("ETH-USD"), InstrumentType::Option)
            .await;
        let live_instruments = common_io.get_instrument_repo().get_instrument_by_type_live(InstrumentType::Option).await;
        match live_instruments {
            Ok(instruments) => {
                // update in-memory inst_ids
                Self::update_instruments(share_instruments, instruments.clone())?;
                Ok(instruments)
            }
            Err(err) => {
                log::error!("skip refresh failed to query okx instruments: {:?}", err);
                Err(err)
            }
        }
    }

    ///
    /// 初始化所有的candle。首先获取所有的instrument，然后获取每个instrument的candle。然后存入数据库。
    ///
    /// # candle的时间判断。
    /// - 开始时间：按照下面的优先级取到值，然后和earliest_timestamp取最大值，来获取，然后+1
    ///   - 从okx_kline里面inst_id中最大的timestamp。
    ///   - OKX_INSTRUMENTS中的listTime
    ///   - Unix timestamp
    ///
    ///
    /// - 结束时间: interval最近的时间戳+1
    ///
    pub async fn initial_candle(&self, earliest_timestamp: UnixTimeStamp, max_sync: Option<usize>) -> Result<(), YuError> {
        // 先从 RwLock 中克隆出一份 Vec，避免持有读锁跨 await，确保 Send
        let inst_vec = self
            .instruments
            .read()
            .map_err(|e| YuError::CustomError(format!("failed to acquire inst_ids read lock: {:?}", e)))?
            .clone();
        let total = inst_vec.len();
        let kline_repo = self.common_io.get_kline_repo();
        let max_timestamp_mapping = kline_repo.max_timestamp_group_by_inst_id().await?;
        // clone into Arc so it can be cheaply shared into async tasks
        let max_ts_map = std::sync::Arc::new(max_timestamp_mapping);
        let interval = self.interval.clone();
        let end = interval.get_now_close_unix_ms_utc() + 10;
        info!("开始初始化，okx option k线，需要同步数量: {}", total);

        // 并发控制
        let concurrency = max_sync.unwrap_or(10).max(1);
        let common_io = self.common_io.clone();
        let earliest = earliest_timestamp;
        // 并发拉取，每个任务返回 Result<Vec<OkxKlinePo>, YuError>
        let stream = futures::stream::iter(inst_vec.into_iter().map(move |inst| {
            let common_io = common_io.clone();
            let interval = interval.clone();
            let max_ts_map = max_ts_map.clone();
            let end = end;
            let earliest = earliest;
            async move {
                let latest_timestamp = max_ts_map.get(&inst.id).cloned().unwrap_or(inst.list_time.unwrap_or(0));
                let start = interval.get_close_unix_ms(std::cmp::max(latest_timestamp, earliest)) + 1;
                let gap = interval.to_milliseconds();
                if end <= start || (end.saturating_sub(start) < gap) {
                    debug!(
                        "Skipping fetch_history for {} as the time gap is too small: end={}, start={}, window_ms={}",
                        inst.inst_identify,
                        unix_2_readable(&end),
                        unix_2_readable(&start),
                        gap
                    );
                    return Ok(vec![]);
                }

                common_io
                    .fetch_history(inst.inst_identify.as_ref(), inst.id, start, end, &interval, None, None)
                    .await
            }
        }))
        .buffer_unordered(concurrency);

        // 收集结果并在遇到第一个错误时返回，同时打印进度
        let mut any_error: Option<YuError> = None;
        futures::pin_mut!(stream);
        let mut completed: usize = 0;
        while let Some(res) = stream.next().await {
            completed = completed.saturating_add(1);
            match res {
                Ok(_) => {
                    let pct = if total > 0 { (completed as f64 / total as f64) * 100.0 } else { 100.0 };
                    info!("initial_candle progress: {}/{} ({:.1}%)", completed, total, pct);
                }
                Err(e) => {
                    error!("initial_candle task failed at {}/{}: {:?}", completed, total, e);
                    any_error = Some(e);
                    break;
                }
            }
        }

        if let Some(e) = any_error {
            return Err(e);
        }

        info!("完成初始化，okx option k线");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{CommonIOService, KlineHandler, MockCommonIOServiceTrait, OptionService, fetch_and_update_instruments, fetch_history};
    use crate::errors::YuError;
    use crate::okx::duck_po::InstrumentPo;
    use crate::okx::duckdb_repository::OkxInstrumentRepository;
    use crate::okx::duckdb_repository::{MockOkxInstrumentRepositoryTrait, MockOkxKlineRepositoryTrait, OkxKlineRepository};
    use crate::okx::duckdb_tables::initial_okx_tables;
    use crate::okx::okx_consts::InstrumentType;
    use crate::test_utils::create_memory_db_provider;
    use li::websocket::connection::MessageHandlerTrait;
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};
    use yue::models::HistoryInterval;
    use yue::okx::models::common::{CandleResponse, InstrumentInfo, OkxListResponse};
    use yue::okx::models::websocket::{ArgBody, KlinePayload, OkxWebsocketResponse};
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
            .withf(|instrument_info| instrument_info.inst_identify == "BTC-1")
            .returning(|_| Ok(()));

        let inst_repo: OkxInstrumentRepository = Arc::new(mock_inst_repo);
        let api: OKxApi = Arc::new(mock_api);
        fetch_and_update_instruments(param, InstrumentType::Option, &inst_repo, &api)
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
            .id(123)
            .inst_identify("BTC-1".to_string())
            .inst_type("OPTION".to_string())
            .base_ccy("BTC".to_string())
            .state("live".to_string())
            .build();
        let db_btc2 = InstrumentPo::builder()
            .id(789)
            .inst_identify("BTC-2".to_string())
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
            .withf(|instrument_info: &InstrumentPo| instrument_info.inst_identify == "BTC-2" && instrument_info.state == Some("live".to_string()))
            .returning(|_| Ok(()));

        let inst_repo: OkxInstrumentRepository = Arc::new(mock_inst_repo);
        let api: OKxApi = Arc::new(mock_api);

        fetch_and_update_instruments(param, InstrumentType::Option, &inst_repo, &api)
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

        let res = fetch_history("btc-usd", 123, start, end, &interval, &api, &kline_repo, Some(2), None).await?;
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

        let res = fetch_history("btc-usd", 123, start, end, &interval, &api, &kline_repo, Some(2), None).await?;
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

        let res = fetch_history("btc-usd", 123, start, end, &interval, &api, &kline_repo, Some(2), Some(2)).await;
        match res {
            Err(YuError::MaxErrorReached(msg, cnt)) => {
                assert!(msg.contains("fetch okx kline net error"));
                // first two errors are retried, third should trigger MaxErrorReached -> cnt == 3
                assert_eq!(cnt, 3);
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }
    fn link_mock_common_io_service(
        mut common_io: MockCommonIOServiceTrait,
        mock_instrument_repo: MockOkxInstrumentRepositoryTrait,
        mock_kline_repo: MockOkxKlineRepositoryTrait,
        mock_api: MockOKXApiTrait,
    ) -> CommonIOService {
        let arc_mock_inst_repo: OkxInstrumentRepository = Arc::new(mock_instrument_repo);
        let arc_mock_kline_repo: OkxKlineRepository = Arc::new(mock_kline_repo);
        let arc_mock_api: OKxApi = Arc::new(mock_api);
        common_io.expect_get_instrument_repo().return_const(arc_mock_inst_repo);
        common_io.expect_get_kline_repo().return_const(arc_mock_kline_repo);
        common_io.expect_get_okx_api().return_const(arc_mock_api);
        Arc::new(common_io)
    }

    ///
    /// 在kline里面，没有数据。
    /// 那么，应该是1，取instrument的里面的list time
    ///
    #[tokio::test]
    pub async fn test_option_service_initial_kline_empty() {
        let start_time = HistoryInterval::OneHour.to_milliseconds() + 1;

        let mock_instrument_repo = MockOkxInstrumentRepositoryTrait::new();
        let mut mock_kline_repo = MockOkxKlineRepositoryTrait::new();
        mock_kline_repo.expect_max_timestamp_group_by_inst_id().returning(|| Ok(HashMap::new()));

        let mock_api = MockOKXApiTrait::new();
        let mut mock_common_io = MockCommonIOServiceTrait::new();
        let expected_start = start_time;
        mock_common_io
            .expect_fetch_history()
            .times(1)
            .withf(move |inst_id, _, start_ts, _, _, _, _| inst_id == "BTC1" && start_ts == &expected_start)
            .return_once(|_, _, _, _, _, _, _| Ok(vec![]));

        let common_io: CommonIOService = link_mock_common_io_service(mock_common_io, mock_instrument_repo, mock_kline_repo, mock_api);

        let inst_po = InstrumentPo::builder()
            .id(123)
            .inst_identify("BTC1".to_string())
            .inst_type("OPTION".to_string())
            .base_ccy("BTC".to_string())
            .list_time(start_time)
            .build();
        let option_service = OptionService::new_with_mock(Arc::new(RwLock::new(vec![inst_po])), common_io, HistoryInterval::OneHour);

        let res = option_service.initial_candle(0, None).await;
        assert!(res.is_ok());
    }

    ///
    /// 在kline里面，有数据
    /// 那么，应该是1，取instrument的里面的list time
    ///
    #[tokio::test]
    pub async fn test_option_service_initial_kline_has_value() {
        let mock_instrument_repo = MockOkxInstrumentRepositoryTrait::new();
        let mut mock_kline_repo = MockOkxKlineRepositoryTrait::new();
        mock_kline_repo.expect_max_timestamp_group_by_inst_id().returning(|| {
            let mut res = HashMap::<u64, u64>::new();
            res.insert(123, HistoryInterval::OneHour.to_milliseconds() * 2 + 1);
            Ok(res)
        });
        let mock_api = MockOKXApiTrait::new();
        let mut mock_common_io = MockCommonIOServiceTrait::new();
        let expected_start = HistoryInterval::OneHour.to_milliseconds() * 2 + 1;
        mock_common_io
            .expect_fetch_history()
            .times(1)
            .withf(move |inst_id, _, start_ts, _, _, _, _| inst_id == "BTC1" && start_ts == &expected_start)
            .return_once(|_, _, _, _, _, _, _| Ok(vec![]));

        let common_io: CommonIOService = link_mock_common_io_service(mock_common_io, mock_instrument_repo, mock_kline_repo, mock_api);

        let inst_po = InstrumentPo::builder()
            .id(123)
            .inst_identify("BTC1".to_string())
            .inst_type("OPTION".to_string())
            .base_ccy("BTC".to_string())
            .list_time(HistoryInterval::OneHour.to_milliseconds() + 1)
            .build();
        let option_service = OptionService::new_with_mock(Arc::new(RwLock::new(vec![inst_po])), common_io, HistoryInterval::OneHour);

        let res = option_service.initial_candle(0, None).await;
        assert!(res.is_ok());
    }

    ///
    /// 在kline里面，有数据
    /// 那么，应该是1，取instrument的里面的list time
    ///
    #[tokio::test]
    pub async fn test_option_service_initial_max_timestamp() {
        let mock_instrument_repo = MockOkxInstrumentRepositoryTrait::new();
        let mut mock_kline_repo = MockOkxKlineRepositoryTrait::new();
        mock_kline_repo.expect_max_timestamp_group_by_inst_id().returning(|| {
            let mut res = HashMap::<u64, u64>::new();
            res.insert(123, HistoryInterval::OneHour.to_milliseconds() * 2 + 1);
            Ok(res)
        });
        let mock_api = MockOKXApiTrait::new();
        let mut mock_common_io = MockCommonIOServiceTrait::new();
        let expected_start = HistoryInterval::OneHour.to_milliseconds() * 3 + 1;
        mock_common_io
            .expect_fetch_history()
            .times(1)
            .withf(move |inst_id, _, start_ts, _, _, _, _| inst_id == "BTC1" && start_ts == &expected_start)
            .return_once(|_, _, _, _, _, _, _| Ok(vec![]));

        let common_io: CommonIOService = link_mock_common_io_service(mock_common_io, mock_instrument_repo, mock_kline_repo, mock_api);

        let inst_po = InstrumentPo::builder()
            .id(123)
            .inst_identify("BTC1".to_string())
            .inst_type("OPTION".to_string())
            .base_ccy("BTC".to_string())
            .list_time(HistoryInterval::OneHour.to_milliseconds() + 1)
            .build();
        let option_service = OptionService::new_with_mock(Arc::new(RwLock::new(vec![inst_po])), common_io, HistoryInterval::OneHour);

        let res = option_service.initial_candle(HistoryInterval::OneHour.to_milliseconds() * 3, None).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    pub async fn test_option_kline_handler() {
        let mut mock_kline_repo = MockOkxKlineRepositoryTrait::new();
        mock_kline_repo
            .expect_insert_history()
            .times(1)
            .withf(|po| po.ts == 111)
            .returning(|_| Ok(()));
        let mock_inst_repo = MockOkxInstrumentRepositoryTrait::new();
        let inst_po = InstrumentPo::builder()
            .id(123)
            .inst_identify("BTC-1".to_string())
            .inst_type("OPTION".to_string())
            .base_ccy("BTC".to_string())
            .list_time(HistoryInterval::OneHour.to_milliseconds() + 1)
            .build();
        let empty_map = HashMap::from([("BTC-1".to_string(), inst_po)]);

        let handler = KlineHandler::new(Arc::new(mock_kline_repo), Arc::new(mock_inst_repo), empty_map);

        let arg_body = ArgBody::builder().inst_id("BTC-1".to_string()).channel("111".to_string()).build();
        let confirm_data = vec![
            "111".to_string(),
            "222".to_string(),
            "333".to_string(),
            "4444".to_string(),
            "555".to_string(),
            "666".to_string(),
            "777".to_string(),
            "888".to_string(),
            "1".to_string(),
        ];
        let un_confirm_data = vec![
            "0".to_string(),
            "222".to_string(),
            "333".to_string(),
            "4444".to_string(),
            "555".to_string(),
            "666".to_string(),
            "777".to_string(),
            "888".to_string(),
            "0".to_string(),
        ];
        let confirm_response_play = KlinePayload::builder()
            .arg(arg_body.clone())
            .data(vec![confirm_data, un_confirm_data])
            .build();

        handler.handle_message(&OkxWebsocketResponse::Kline(confirm_response_play)).await;
    }
}
