use governor::RateLimiter;
use governor::clock::DefaultClock;
use governor::middleware::{StateInformationMiddleware, StateSnapshot};
use governor::state::{InMemoryState, NotKeyed};
use governor::{Jitter, Quota};
use li::tools::time::{UnixTimeStamp, unix_time_now_u64_utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::timeout;
use url::Url;

///
/// PLAN：去除交易所之类的类的定义，因为发现这样没有办法统一

//不太确定哪个好，就先用这个用于高精度计算
pub type Decimal = rust_decimal::Decimal;
#[macro_export]
macro_rules! dec_from_opt_f64 {
    ($e:expr) => {
        crate::models::Decimal::from_f64($e).unwrap_or_default()
    };
}

pub fn create_empty_param() -> Option<EmptyObject> {
    Option::from(EmptyObject {})
}
#[derive(Debug, PartialEq, Default)]
pub struct EmptyObject;

impl std::fmt::Display for EmptyObject {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "")
    }
}

impl Serialize for EmptyObject {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let empty_map: HashMap<String, serde_json::Value> = HashMap::new();
        empty_map.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EmptyObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let empty_map: HashMap<String, serde_json::Value> = HashMap::deserialize(deserializer)?;
        if empty_map.is_empty() {
            Ok(EmptyObject {})
        } else {
            Err(de::Error::custom("Expected an empty JSON object"))
        }
    }
}

pub type DefaultRateLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock, StateInformationMiddleware>;
pub type ShareRateLimiter = Arc<RwLock<Arc<DefaultRateLimiter>>>;
///
/// 主要是参考币安的经验。这个HostInfo主要是和限流和地址相关信息。
///
/// 1. 主要提供方法。
///
/// 1. 获得host，直接把HostInfo转换成host，作为String输出
/// 2. 提供更新max_limit和获得max_limit以u32形式的方法
/// 3. 更新和获得limiter的方法。以及获得令牌的方法。
///
///
#[derive(Debug, Clone)]
pub struct HostInfo {
    host: String,
    max_limit: Arc<AtomicU32>,
    disable_before: Arc<AtomicU64>, //unix timestamp,在此时间点之前，该host应该不可用。
    limiter: ShareRateLimiter,
}

pub fn create_share_rate_limiter(bucket_size: u32) -> ShareRateLimiter {
    Arc::new(RwLock::new(Arc::new(create_default_rate_limiter(bucket_size))))
}

pub fn create_default_rate_limiter(bucket_size: u32) -> DefaultRateLimiter {
    let quota = Quota::per_minute(NonZeroU32::new(bucket_size).unwrap());
    let res = RateLimiter::direct(quota);
    let limiter_with_info = res.with_middleware::<StateInformationMiddleware>();
    limiter_with_info
}

impl AsRef<str> for HostInfo {
    fn as_ref(&self) -> &str {
        &self.host
    }
}

impl<'a> From<&'a HostInfo> for &'a str {
    fn from(value: &'a HostInfo) -> Self {
        &value.host
    }
}

impl HostInfo {
    pub fn new<S: AsRef<str>>(host: S, max_limit: u32, limiter: Arc<RwLock<Arc<DefaultRateLimiter>>>) -> Self {
        Self {
            host: host.as_ref().to_string(),
            max_limit: Arc::new(AtomicU32::new(max_limit)),
            disable_before: Arc::new(AtomicU64::new(0)),
            limiter,
        }
    }

    ///
    /// 并等到到host重新open
    ///
    /// reopen_timestamp: 设置host重新开启的时间时间戳。如果为None，表示不需要设置，如果其他线程设置，就等待。
    /// jitter：时间抖动。默认为2分钟。
    ///
    /// # 方法逻辑。
    /// 1. 如果reopen_timestamp为None，
    ///     1. 现在的时间戳大于self.disable_before，认为host是开的，直不用等待。
    ///     2. 现在的时间戳小于self.disable_before，等到到disable_before+jitter
    /// 2. 比较reopen_timestamp不为None。那么以下情况
    ///     1.reopen_timestamp > disable_before
    ///         1. 设置disable_before为reopen_timestamp
    ///         2. 等待reopen_timestamp+jitter的时间
    ///     2.reopen_timestamp < disable_before
    ///         2. disable_before+jitter的时间
    /// 3. 等待结束后，检查等待结束的时间戳end_timestamp和disable_before的关系， 如果end_timestamp < disable_before，
    ///    说明host还是关闭的，那么继续等待，直到end_timestamp > disable_before
    pub fn waiting_for_open(&self, reopen_timestamp: Option<UnixTimeStamp>, jitter: Option<Jitter>) {
        // 该方法为同步阻塞实现：等待直到 host 被标记为 open（disable_before 小于当前时间）
        // 设计要点：
        // 1. 如果 reopen_timestamp 为 None，且当前时间 > disable_before，则立即返回（host 已开启）
        // 2. 如果 reopen_timestamp 有值且大于当前的 disable_before，则把 disable_before 更新为 reopen_timestamp
        // 3. 否则（reopen_timestamp < disable_before 或 reopen_timestamp 为 None），以当前 disable_before 为等待目标
        // 4. 通过循环检查并短时间 sleep，直到当前时间超过目标时间
        // 说明：
        // - 由于函数为同步接口，使用 std::thread::sleep 做短轮询，避免 busy-spin
        // - jitter 参数目前仅作为提示：若提供则在等待目标上额外增加少量缓冲（取 0..max_jitter/2），以避免临界竞争。

        // 安全读取当前时间和 disable_before
        let mut now = unix_time_now_u64_utc();
        let mut disable_before = self.disable_before.load(Ordering::Relaxed);

        // 快速路径：无 reopen_timestamp 并且已经开了
        if reopen_timestamp.is_none() && now > disable_before {
            return;
        }

        // 如果传入 reopen_timestamp 并且大于当前 disable_before，则进行更新
        if let Some(reopen_ts) = reopen_timestamp {
            if reopen_ts > disable_before {
                self.disable_before.store(reopen_ts, Ordering::SeqCst);
                disable_before = reopen_ts;
            }
        }
        let real_jitter: Jitter = jitter.unwrap_or(Jitter::up_to(Duration::from_secs(120)));
        let buff_duration_ms = (real_jitter + Duration::ZERO).as_millis() as u64;
        // 计算可能的 jitter buffer（取较小的默认值以便测试快速），如果传入 jitter，则使用 100 ms 的上限

        // 目标时间点为 disable_before + jitter_buffer_ms
        let mut target = disable_before.saturating_add(buff_duration_ms);

        // 若当前时间已经超过目标，则直接返回
        now = unix_time_now_u64_utc();
        if now > target {
            return;
        }

        // 循环等待直到当前时间超过目标时间。在循环中重新读取 disable_before，允许其他线程变更该值。
        loop {
            // 每次循环重新读取最新的 disable_before
            let current_disable = self.disable_before.load(Ordering::Relaxed);
            if current_disable > disable_before {
                // 如果有人把 disable_before 提高了，则把目标提升到新的值
                disable_before = current_disable;
                target = disable_before.saturating_add(buff_duration_ms);
            }

            let now = unix_time_now_u64_utc();
            if now > target {
                break;
            }

            // 计算剩余等待 ms，限制最小为 1 ms，最大为 200 ms，避免长时间 sleep
            let remaining_ms = target.saturating_sub(now);
            std::thread::sleep(Duration::from_millis(remaining_ms));
        }
    }

    pub fn get_open_timestamp(&self) -> UnixTimeStamp {
        self.disable_before.load(Ordering::Relaxed)
    }

    pub fn host_as_str(&self) -> &str {
        &self.host
    }

    pub fn set_max_limit(&self, v: u32) {
        self.max_limit.store(v, Ordering::SeqCst);
    }

    pub fn get_max_limit(&self) -> u32 {
        self.max_limit.load(Ordering::SeqCst)
    }

    pub async fn set_limiter(&self, limiter: DefaultRateLimiter) {
        let mut guard = self.limiter.write().await;
        *guard = Arc::new(limiter);
    }

    pub async fn refresh_rate_limit(&self, rate_limit: u32) {
        self.set_limiter(create_default_rate_limiter(rate_limit)).await;
        self.set_max_limit(rate_limit);
    }

    ///
    /// 获取令牌的流程。
    ///
    /// 1. 判断weight是否合法。非零。
    /// 2. 获令牌，获取成功就返回。
    /// 3. 如果没有获取成功，就等待，直到获取成功或者超时。
    /// 4. 最后如果block为0.那么可以获得令牌，如果为1.则拒绝获得令牌。
    ///
    /// # 说明
    /// 1. 返回值定为StateSnapshot，主要是为了以后追求极致性能，去掉
    ///
    pub async fn acquire_limit_token(&self, weight: u32, timeout_secs: u32) -> Result<Option<StateSnapshot>, crate::errors::YueError> {
        // 1. 判断 weight 是否为非零
        let weight_nz = match NonZeroU32::new(weight) {
            Some(w) => w,
            None => return Err(crate::errors::YueError::new("权重必须为非零")),
        };

        // clone an Arc handle to the limiter so we don't hold the RwLock across .await
        let limiter_cloned = {
            let guard = self.limiter.read().await; // tokio RwLock: await to acquire
            guard.clone()
        };

        // 3. 先尝试非阻塞获取（快速失败/成功）
        // 使用带零超时的 until_n_ready_with_jitter 来确保即时获取会消费令牌，
        // 避免依赖 governor 的 check_n 语义（有些版本可能只是检查不消费）。
        let jitter = Jitter::up_to(Duration::from_millis(500));
        let immediate_try = timeout(Duration::from_millis(0), limiter_cloned.until_n_ready_with_jitter(weight_nz, jitter)).await;
        if let Ok(Ok(snapshot)) = immediate_try {
            return Ok(Some(snapshot));
        }

        // 4. 若未立即获取成功，则等待直到超时
        let timeout_duration = Duration::from_secs(timeout_secs as u64);

        match timeout(
            timeout_duration,
            limiter_cloned.until_n_ready_with_jitter(weight_nz, Jitter::up_to(Duration::from_millis(500))),
        )
        .await
        {
            Err(_) => Err(crate::errors::YueError::new("限流超时")),
            Ok(res) => match res {
                Ok(snapshot) => Ok(Some(snapshot)),
                Err(e) => Err(crate::errors::YueError::new(&format!("限流器内部错误: {:?}", e))),
            },
        }
    }
}

/// NEXT：加入一个generate方法，参数为weight和timeout
/// 因为这两个可能是多变的。比如在获取swap kline的过程中，会根据limit进行变更
#[derive(Debug, Clone)]
pub struct RequestInfo {
    inner: Url,
    pub host: Arc<HostInfo>,
    pub has_security: bool,
    pub weight: u32,
    pub request_timeout_mill_secs: u32,
    rate_limit_timeout_secs: u32,
}

impl RequestInfo {
    // 直接从完整 URL 构建
    pub fn new_full_url<S: AsRef<str>>(
        full_url: S,
        host: Arc<HostInfo>,
        has_security: bool,
        weight: u32,
        request_timeout_mill_secs: Option<u32>,
        rate_limit_timeout_secs: Option<u32>,
    ) -> Result<Self, url::ParseError> {
        let inner = Url::parse(full_url.as_ref())?;
        Ok(Self {
            inner,
            host,
            has_security,
            weight,
            request_timeout_mill_secs: request_timeout_mill_secs.unwrap_or_else(|| 1000u32) * 1000u32,
            rate_limit_timeout_secs: rate_limit_timeout_secs.unwrap_or_else(|| 2),
        })
    }

    // 从 base + path 构建（内部负责安全拼接）
    pub fn from_base_path<P: AsRef<str>>(
        host: Arc<HostInfo>,
        path: P,
        has_security: bool,
        weight: u32,
        request_timeout_secs: Option<u32>,
        rate_limit_timeout_secs: Option<u32>,
    ) -> Result<Self, url::ParseError> {
        let base = host.host_as_str();
        let path = path.as_ref();
        let full = if path.starts_with('/') {
            format!("{base}{path}")
        } else {
            format!("{base}/{path}")
        };
        Self::new_full_url(full, host, has_security, weight, request_timeout_secs, rate_limit_timeout_secs)
    }

    pub fn clone_with_weight(&mut self, new_weight: u32) -> Self {
        Self {
            inner: self.inner.clone(),
            host: self.host.clone(),
            has_security: self.has_security,
            weight: new_weight,
            request_timeout_mill_secs: self.request_timeout_mill_secs,
            rate_limit_timeout_secs: self.rate_limit_timeout_secs,
        }
    }

    // 如需获取内部 Url 的只读引用
    pub fn url(&self) -> &Url {
        &self.inner
    }

    // 字符串视图
    pub fn as_str(&self) -> &str {
        self.inner.as_str()
    }

    pub fn get_rate_limit_timeout(&self) -> u32 {
        self.rate_limit_timeout_secs
    }
}

impl AsRef<Url> for RequestInfo {
    fn as_ref(&self) -> &Url {
        &self.inner
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HistoryInterval {
    OneMinute,
    FiveMinutes,
    OneHour,
}

impl AsRef<str> for HistoryInterval {
    fn as_ref(&self) -> &str {
        match self {
            HistoryInterval::OneMinute => "1m",
            HistoryInterval::FiveMinutes => "5m",
            HistoryInterval::OneHour => "1h",
        }
    }
}

impl HistoryInterval {
    pub fn to_milliseconds(&self) -> u64 {
        match self {
            HistoryInterval::OneMinute => 60 * 1000,
            HistoryInterval::FiveMinutes => 5 * 60 * 1000,
            HistoryInterval::OneHour => 60 * 60 * 1000,
        }
    }

    ///
    /// 获得传入一个时间戳，最近的时间符合的时间unix mill second
    /// 比如传入 10:12:33
    /// 那么
    /// 1m: 返回 10:12:00的 unix ms
    /// 5m: 返回 10:10:00的 unix ms
    /// 1h: 返回 10:00:00的 unix ms
    ///
    pub fn get_close_unix_ms(&self, timestamp: u64) -> u64 {
        // 获取当前时间的 unix 毫秒，若出错则返回 0
        let interval_ms = self.to_milliseconds();
        // 向下取整到 interval 边界
        (timestamp / interval_ms) * interval_ms
    }

    pub fn get_now_close_unix_ms_utc(&self) -> u64 {
        self.get_close_unix_ms(unix_time_now_u64_utc())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{HistoryInterval, HostInfo, create_share_rate_limiter};
    use governor::Jitter;
    use li::tools::time::unix_time_now_u64_utc;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    /// 测试：HistoryInterval::get_close_unix_ms 在一分钟间隔下的对齐
    ///
    /// 设计思路：验证get_close_unix_ms能够正确将任意时间戳对齐到interval边界
    ///
    /// 场景说明：
    /// - 传入当前时间戳
    /// - 验证返回的时间戳对齐到1分钟边界
    /// - 返回的时间戳不应超过传入的时间戳
    /// - 两者间的差距应小于1分钟
    #[test]
    fn test_get_close_unix_ms_one_minute() {
        let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let ts = HistoryInterval::OneMinute.get_close_unix_ms(now_ms);
        assert!(ts <= now_ms, "返回的时间不应在未来");
        assert_eq!(ts % (60 * 1000), 0, "应对齐到整分钟");
        assert!(now_ms - ts < 60 * 1000, "差距应小于 1 分钟");
    }

    /// 测试：HistoryInterval::get_close_unix_ms 在五分钟间隔下的对齐
    ///
    /// 设计思路：验证get_close_unix_ms能够正确将任意时间戳对齐到5分钟边界
    ///
    /// 场景说明：
    /// - 传入当前时间戳
    /// - 验证返回的时间戳对齐到5分钟边界
    /// - 返回的时间戳不应超过传入的时间戳
    /// - 两者间的差距应小于5分钟
    #[test]
    fn test_get_close_unix_ms_five_minutes() {
        let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let ts = HistoryInterval::FiveMinutes.get_close_unix_ms(now_ms);
        assert!(ts <= now_ms, "返回的时间不应在未来");
        assert_eq!(ts % (5 * 60 * 1000), 0, "应对齐到 5 分钟边界");
        assert!(now_ms - ts < 5 * 60 * 1000, "差距应小于 5 分钟");
    }

    /// 测试：HistoryInterval::get_close_unix_ms 在一小时间隔下的对齐
    ///
    /// 设计思路：验证get_close_unix_ms能够正确将任意时间戳对齐到1小时边界
    ///
    /// 场景说明：
    /// - 传入当前时间戳
    /// - 验证返回的时间戳对齐到1小时边界
    /// - 返回的时间戳不应超过传入的时间戳
    /// - 两者间的差距应小于1小时
    #[test]
    fn test_get_close_unix_ms_one_hour() {
        let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let ts = HistoryInterval::OneHour.get_close_unix_ms(now_ms);
        assert!(ts <= now_ms, "返回的时间不应在未来");
        assert_eq!(ts % (60 * 60 * 1000), 0, "应对齐到整小时");
        assert!(now_ms - ts < 60 * 60 * 1000, "差距应小于 1 小时");
    }

    /// 测试 HostInfo 的基本行为与限流令牌获取（基础场景）
    ///
    /// 目的：验证 HostInfo 的字段访问与基础限流获取行为。
    /// 场景：
    /// - 使用每分钟 100 个令牌的 quota
    /// - 验证 host 字符串与 max_limit 的读写行为
    /// - 初次调用 acquire_limit_token(1, 2) 应成功（令牌充足）
    /// 期望：首次获取令牌返回 Ok
    #[tokio::test]
    async fn test_host_info_basic_and_acquire() {
        let limiter = create_share_rate_limiter(100);

        let host = HostInfo::new("https://api.test", 1000, limiter);

        assert_eq!(host.host_as_str(), "https://api.test");
        assert_eq!(host.get_max_limit(), 1000);

        host.set_max_limit(500);
        assert_eq!(host.get_max_limit(), 500);

        // acquire should succeed for weight 1
        let res = host.acquire_limit_token(1, 2).await;
        assert!(res.is_ok(), "首次获取令牌应成功");
    }

    /// 测试限流超时行为
    ///
    /// 目的：验证在严格配额下，第二次快速请求会因为令牌未补满而超时失败。
    /// 场景：
    /// - 使用每分钟 1 个令牌的 quota（非常低的配额）
    /// - 第一次调用 acquire_limit_token(1, 1) 应成功并消耗该令牌
    /// - 第二次在短超时时间内再次调用应返回 Err（超时或拒绝）
    /// - 当 weight 为 0 时，应当立即返回错误
    /// 期望：第一次 Ok，第二次 Err，weight=0 Err
    #[tokio::test]
    async fn test_acquire_timeout_behavior() {
        let limiter = create_share_rate_limiter(1);

        let host = HostInfo::new("https://api.test", 10, limiter);

        // first acquire should succeed
        let r1 = host.acquire_limit_token(1, 1).await;
        assert!(r1.is_ok(), "第一次获取令牌应成功");

        // second acquire with short timeout should fail due to token refill being long
        let r2 = host.acquire_limit_token(1, 1).await;
        assert!(r2.is_err(), "在短超时时间内第二次获取应失败");

        // zero weight should return error
        let r3 = host.acquire_limit_token(0, 1).await;
        assert!(r3.is_err(), "权重为0应当报错");
    }

    /// 测试目的：当 `disable_before` 在过去时，`waiting_for_open(None, None)` 应立即返回。
    /// 场景：将 `disable_before` 设置为当前时间之前（表示 host 已可用），调用等待函数不应阻塞。
    /// 断言：耗时非常短（<50ms），以确保没有进行不必要的等待。
    #[test]
    fn test_waiting_for_open_returns_immediately_if_open() {
        let limiter = create_share_rate_limiter(10);
        let host = HostInfo::new("https://api.test", 10, limiter);

        // 确保 disable_before 在过去
        host.disable_before.store(unix_time_now_u64_utc().saturating_sub(1000), Ordering::SeqCst);

        let start = Instant::now();
        host.waiting_for_open(None, None);
        let elapsed = start.elapsed();

        assert!(elapsed.as_millis() < 50, "应当立即返回，实际耗时 {:?}", elapsed);
    }

    /// 测试目的：当 `disable_before` 在将来时，`waiting_for_open(None, None)` 应等待直到该时间到达。
    /// 场景：把 `disable_before` 设为短期未来（约150ms），不传入 `reopen_timestamp` 或 `jitter`。
    /// 断言：函数至少会等待接近该期望（>=140ms），允许少量调度开销误差。
    #[test]
    fn test_waiting_for_open_waits_until_disable_before() {
        let limiter = create_share_rate_limiter(10);
        let host = HostInfo::new("https://api.test", 10, limiter);

        // 设定 disable_before 为短期未来
        let now = unix_time_now_u64_utc();
        host.disable_before.store(now + 150, Ordering::SeqCst);
        let jitter = Jitter::up_to(Duration::from_millis(30));
        let start = Instant::now();
        host.waiting_for_open(None, Some(jitter));
        let elapsed = start.elapsed();

        // 至少等待了约 150ms
        assert!(elapsed.as_millis() >= 140, "应当等待至少 140ms, 实际: {:?}", elapsed);
    }

    /// 测试目的：当传入一个比当前 `disable_before` 更远的 `reopen_timestamp` 时，
    /// `waiting_for_open(Some(reopen_timestamp), Some(jitter))` 应把 `disable_before` 更新为该值并等待到更新后的时间。
    /// 场景：先把 `disable_before` 设为较小的未来时间，再传入一个更靠后的 `reopen_timestamp`（约200ms）。
    /// 断言：函数等待接近更新后的时间（>=180ms），考虑 jitter 与调度误差。
    #[test]
    fn test_waiting_for_open_with_reopen_timestamp_updates_target() {
        let limiter = create_share_rate_limiter(10);
        let host = HostInfo::new("https://api.test", 10, limiter);

        // 将 disable_before 设为一个较小的未来时间
        let now = unix_time_now_u64_utc();
        host.disable_before.store(now + 50, Ordering::SeqCst);
        let jitter = Jitter::up_to(Duration::from_millis(50));
        // 传入更远的 reopen_timestamp，函数应把 disable_before 更新为该值并等待
        let reopen_ts = now + 200;
        let start = Instant::now();
        host.waiting_for_open(Some(reopen_ts), Some(jitter));
        let elapsed = start.elapsed();

        assert!(elapsed.as_millis() >= 180, "应当等待直到 reopen_timestamp (~200ms), 实际: {:?}", elapsed);
    }

    /// 测试目的：当传入的 `reopen_timestamp` 小于当前 `disable_before` 时，等待目标不应被缩短，
    /// 而应仍以已有的 `disable_before` 为准。
    /// 场景：把 `disable_before` 设为较远的未来（约300ms），传入较近的 `reopen_timestamp`（约50ms）。
    /// 断言：函数等待接近原有的 `disable_before`（>=260ms），而非被传入的较小时间所覆盖。
    #[test]
    fn test_waiting_for_open_with_reopen_smaller_than_existing() {
        // 如果传入的 reopen_timestamp 小于已有的 disable_before，应当以已有的 disable_before 为准等待
        let limiter = create_share_rate_limiter(10);
        let host = HostInfo::new("https://api.test", 10, limiter);

        let now = unix_time_now_u64_utc();
        // 现有 disable_before 比 reopen_timestamp 远
        host.disable_before.store(now + 300, Ordering::SeqCst);

        // 传入一个较小的 reopen timestamp
        let reopen_ts = now + 50;
        let start = Instant::now();
        let jitter = Jitter::up_to(Duration::from_millis(50));
        host.waiting_for_open(Some(reopen_ts), Some(jitter));
        let elapsed = start.elapsed();

        // 应该等待接近已有的 300ms，而不是 50ms
        assert!(
            elapsed.as_millis() >= 260,
            "应当等待直到原有 disable_before (~300ms), 实际: {:?}",
            elapsed
        );
    }

    /// 测试目的：验证 `waiting_for_open` 在并发情况下能响应其他线程对 `disable_before` 的提升，
    /// 并据此延长等待时间。
    /// 场景：初始 `disable_before` 设为 100ms，将在另一个线程中于 60ms 后把 `disable_before` 再次提升到更远的时间点。
    /// 断言：原始等待会被延长（>=340ms），表明函数在循环中重新读取并尊重更新后的 `disable_before`。
    #[test]
    fn test_waiting_for_open_concurrent_update_extends_wait() {
        // 并发场景：在等待过程中，另一线程将 disable_before 提高，等待应随之延长
        let limiter = create_share_rate_limiter(10);
        let host = HostInfo::new("https://api.test", 10, limiter);

        let now = unix_time_now_u64_utc();
        host.disable_before.store(now + 100, Ordering::SeqCst);

        // 另起一个线程，在 60ms 后把 disable_before 提高到 now + 400
        let host_clone = host.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            let later = unix_time_now_u64_utc();
            // set to a further future (use relative to original now to keep expectation stable)
            host_clone.disable_before.store(later + 300, Ordering::SeqCst);
        });
        let jitter = Jitter::up_to(Duration::from_millis(50));
        let start = Instant::now();
        host.waiting_for_open(None, Some(jitter));
        let elapsed = start.elapsed();

        // 初始等待 100ms，但因为另一个线程延后了 reopen，应至少等待到 ~360ms
        assert!(elapsed.as_millis() >= 340, "并发更新应延长等待，实际: {:?}", elapsed);
    }
}
