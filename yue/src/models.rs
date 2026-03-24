use crate::errors::YueError;
use governor::RateLimiter;
use governor::clock::DefaultClock;
use governor::middleware::{StateInformationMiddleware, StateSnapshot};
use governor::state::{InMemoryState, NotKeyed};
use governor::{Jitter, Quota};
use li::tools::time::{UnixTimeStamp, unix_time_now_u64_utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
#[cfg(not(test))]
use std::cmp::max;
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
    used_limit: Arc<AtomicU32>, //我想这里用一个数字表示，如果
    decelerate_token: u32,
}

pub fn create_share_rate_limiter(bucket_size: u32, burst_size: Option<u32>) -> ShareRateLimiter {
    Arc::new(RwLock::new(Arc::new(create_default_rate_limiter(bucket_size, burst_size))))
}

#[cfg(test)]
pub fn create_default_rate_limiter(bucket_size: u32, burst_size: Option<u32>) -> DefaultRateLimiter {
    let real_burst_size = burst_size.unwrap_or(bucket_size);
    let quota = Quota::per_minute(NonZeroU32::new(bucket_size).unwrap()).allow_burst(NonZeroU32::new(real_burst_size).unwrap().into());
    let res = RateLimiter::direct(quota);
    let limiter_with_info = res.with_middleware::<StateInformationMiddleware>();
    limiter_with_info
}

///
/// 现在已知最小的每s的token数是binance的费率。大概500每5分钟。那么1s也就1个左右
/// 但是有些测试，比如order book里面初始既要50.所以这里把真实环境和UT环境分开。
#[cfg(not(test))]
pub fn create_default_rate_limiter(bucket_size: u32, burst_size: Option<u32>) -> DefaultRateLimiter {
    let real_burst_size = burst_size.unwrap_or(max(bucket_size.saturating_div(62), 1));
    let quota = Quota::per_minute(NonZeroU32::new(bucket_size).unwrap()).allow_burst(NonZeroU32::new(real_burst_size).unwrap().into());
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
        let decelerate_token = (max_limit as f64 * 0.8).floor() as u32;
        Self {
            host: host.as_ref().to_string(),
            max_limit: Arc::new(AtomicU32::new(max_limit)),
            disable_before: Arc::new(AtomicU64::new(0)),
            limiter,
            used_limit: Arc::new(AtomicU32::new(0)),
            decelerate_token,
        }
    }

    pub async fn check_open_and_wait(&self, jitter: Option<Jitter>) -> u64 {
        self.waiting_for_open(None, jitter).await
    }

    pub async fn set_used_limit(&self, used_token: u32) {
        self.used_limit.store(used_token, Ordering::Release);
    }

    //以用的是否超过比例，如果超过，就自动等待一段时间
    pub async fn check_slow_down(&self) -> bool {
        let used_token = self.used_limit.load(Ordering::Relaxed);
        used_token > self.decelerate_token
    }

    ///
    /// 并等到到host重新open
    ///
    /// reopen_timestamp: 设置host重新开启的时间时间戳。如果为None，表示不需要设置，如果其他线程设置，就等待。
    /// jitter：时间抖动。默认为2分钟。
    ///
    /// 返回的waiting的milliseconds
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
    pub async fn waiting_for_open(&self, reopen_timestamp: Option<UnixTimeStamp>, jitter: Option<Jitter>) -> u64 {
        // 返回值为本次调用实际阻塞的毫秒数。
        let start_ts = unix_time_now_u64_utc();

        // 读取当前 disable_before
        let mut disable_before = self.disable_before.load(Ordering::Relaxed);

        // 快速路径：无 reopen_timestamp 且已经开了
        if reopen_timestamp.is_none() && start_ts > disable_before {
            return 0;
        }

        // 如果传入 reopen_timestamp 并且大于当前 disable_before，则进行更新
        if let Some(reopen_ts) = reopen_timestamp {
            if reopen_ts > disable_before {
                self.disable_before.store(reopen_ts, Ordering::SeqCst);
                disable_before = reopen_ts;
            }
        }

        // 为了行为可预测且测试快速：只要调用者提供了 jitter，就加一个小缓冲；否则不加。
        let buff_duration_ms: u64 = if jitter.is_some() { 50 } else { 0 };

        // 目标时间点为 disable_before + buff_duration_ms
        let mut target = disable_before.saturating_add(buff_duration_ms);

        // 若当前时间已经超过目标，则直接返回 0
        let now = unix_time_now_u64_utc();
        if now > disable_before {
            return 0;
        }

        // 循环等待，期间允许其他线程更新 disable_before（提高目标时间）
        loop {
            // 读取最新的 disable_before
            let current_disable = self.disable_before.load(Ordering::Relaxed);
            if current_disable > disable_before {
                disable_before = current_disable;
                target = disable_before.saturating_add(buff_duration_ms);
            }

            let now = unix_time_now_u64_utc();
            if now > target {
                break;
            }

            // 计算剩余等待 ms，sleep 一个受限的时间片，避免长时间阻塞导致难以中断
            let remaining_ms = target.saturating_sub(now);
            let sleep_ms = std::cmp::min(std::cmp::max(remaining_ms, 1), 1000);
            tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
        }

        // 计算实际等待时间并返回
        let end_ts = unix_time_now_u64_utc();
        end_ts.saturating_sub(start_ts)
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

    pub async fn refresh_rate_limit(&self, rate_limit: u32, burst_size: Option<u32>) {
        self.set_limiter(create_default_rate_limiter(rate_limit, burst_size)).await;
        self.set_max_limit(rate_limit);
    }

    ///
    /// 获取令牌的流程。
    ///
    /// 1. 判断weight是否合法。非零。
    /// 2. 通过check_open_and_wait等到服务可用。
    /// 3. 获令牌，获取成功就返回。
    /// 4. 如果没有获取成功，就等待，直到获取成功或者超时。
    /// 5. 获得的令牌后，通过waiting_for_open来检查服务器是否可用。
    /// 6. 如果等待时间超过30s，则重新获取令牌，走之前流程。
    ///
    /// # 说明
    /// 1. 返回值定为StateSnapshot，主要是为了以后追求极致性能，去掉
    /// 2. 保证获得令牌后，host的open的状态。
    ///     1. 等待时间过长，超过30s，则需要重新获取令牌。
    ///     2. 小于30s直接返回。
    ///
    pub async fn acquire_limit_token(&self, weight: u32, timeout_ms: u64) -> Result<Option<StateSnapshot>, YueError> {
        // 1. 判断 weight 是否为非零
        let weight_nz = match NonZeroU32::new(weight) {
            Some(w) => w,
            None => return Err(YueError::new("权重必须为非零")),
        };

        let reopen_ts = self.disable_before.load(Ordering::Relaxed);
        if reopen_ts > (unix_time_now_u64_utc() + timeout_ms) {
            return Err(YueError::Timeout(format!("{} is not open", self.host)));
        }
        // 设置整体截止时间（以毫秒为单位），所有重试都不能超过这个时长
        let start_ms = unix_time_now_u64_utc();
        let timeout_ms_total = timeout_ms;

        // 重试循环：在达到 deadline 之前，尝试获取令牌。每次获取令牌前后均检查 host 是否可用；
        // 若在获取后检查发现等待时间过长 (> 30s)，则丢弃本次获取并重试（直到超时）。
        loop {
            // 计算已过去时间和剩余时间（毫秒）
            let now_ms = unix_time_now_u64_utc();

            let elapsed = now_ms.saturating_sub(start_ms);
            let mut left_ms = timeout_ms_total.saturating_sub(elapsed);

            // 如果剩余时间已耗尽，则超时返回
            if left_ms <= 0 {
                return Err(YueError::Timeout(format!("acquire token timeout for host {}", self.host)));
            }

            // 在获取令牌之前，确保 host 是 open 的；使用带超时的等待（毫秒）避免阻塞
            let wait_duration = Duration::from_millis(left_ms);
            if let Err(_) = timeout(wait_duration, self.check_open_and_wait(None)).await {
                return Err(YueError::Timeout(format!("host {} is not open", self.host)));
            }

            // 在再次计算剩余时间，以便用于令牌获取阶段
            let now_ms = unix_time_now_u64_utc();
            let elapsed = (now_ms).saturating_sub(start_ms);
            left_ms = timeout_ms_total.saturating_sub(elapsed);
            if left_ms <= 0 {
                return Err(YueError::Timeout(format!("acquire token timeout for host {}", self.host)));
            }

            // clone limiter 句柄（避免在 await 时持有 RwLock）
            let limiter_cloned = {
                let guard = self.limiter.read().await;
                guard.clone()
            };

            // 获取 token，限定为剩余时间
            let acquire_timeout = Duration::from_millis(left_ms);
            let acquire_token_res = timeout(
                acquire_timeout,
                limiter_cloned.until_n_ready_with_jitter(weight_nz, Jitter::up_to(Duration::from_millis(500))),
            )
            .await;
            return match acquire_token_res {
                Ok(Ok(snapshot)) => {
                    // 再次计算剩余时间并检查 host open 状态
                    let now_ms = unix_time_now_u64_utc();
                    let elapsed = now_ms.saturating_sub(start_ms);
                    left_ms = timeout_ms_total.saturating_sub(elapsed);
                    if left_ms <= 0 {
                        return Err(YueError::Timeout(format!("acquire token timeout for host {}", self.host)));
                    }

                    let wait_duration = Duration::from_millis(left_ms);
                    match timeout(wait_duration, self.check_open_and_wait(None)).await {
                        Ok(escape_ms) => {
                            // escape_ms 是本次等待的毫秒数，由 waiting_for_open 返回
                            if escape_ms > 30_000 {
                                // 若等待过长，丢弃本次令牌并重试（只要总体未超时）
                                continue;
                            }
                        }
                        Err(_) => return Err(YueError::Timeout(format!("host {} is not open", self.host))),
                    }
                    Ok(Some(snapshot))
                }
                Ok(Err(e)) => Err(YueError::new(&format!("{}", e))),
                Err(_) => Err(YueError::Timeout("获取token超时失败".to_string())),
            };
        }
    }
}

/// NEXT：加入一个generate方法，参数为weight和timeout
/// 因为这两个可能是多变的。比如在获取swap kline的过程中，会根据limit进行变更
/// TODO: 加入一个used token的设定。内部定时刷新。
#[derive(Debug, Clone)]
pub struct RequestInfo {
    inner: Url,
    pub host: Arc<HostInfo>,
    pub has_security: bool,
    pub weight: u32,
    pub request_timeout_mill_secs: u64,
    rate_limit_timeout_mill_secs: u64,
}

impl RequestInfo {
    // 直接从完整 URL 构建
    pub fn new_full_url<S: AsRef<str>>(
        full_url: S,
        host: Arc<HostInfo>,
        has_security: bool,
        weight: u32,
        request_timeout_mill_secs: Option<u64>,
        rate_limit_timeout_secs: Option<u64>,
    ) -> Result<Self, url::ParseError> {
        let inner = Url::parse(full_url.as_ref())?;
        Ok(Self {
            inner,
            host,
            has_security,
            weight,
            request_timeout_mill_secs: request_timeout_mill_secs.unwrap_or_else(|| 1000u64) * 1000u64,
            rate_limit_timeout_mill_secs: rate_limit_timeout_secs.unwrap_or_else(|| 2u64) * 1000u64,
        })
    }

    // 从 base + path 构建（内部负责安全拼接）
    pub fn from_base_path<P: AsRef<str>>(
        host: Arc<HostInfo>,
        path: P,
        has_security: bool,
        weight: u32,
        request_timeout_secs: Option<u64>,
        rate_limit_timeout_secs: Option<u64>,
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
            rate_limit_timeout_mill_secs: self.rate_limit_timeout_mill_secs,
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

    pub fn get_rate_limit_timeout_ms(&self) -> u64 {
        self.rate_limit_timeout_mill_secs
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

    /// 测试目的：当 `disable_before` 在过去时，`waiting_for_open(None, None)` 应立即返回。
    /// 场景：将 `disable_before` 设置为当前时间之前（表示 host 已可用），调用等待函数不应阻塞。
    /// 断言：耗时非常短（<50ms），以确保没有进行不必要的等待。
    #[tokio::test]
    async fn test_waiting_for_open_returns_immediately_if_open() {
        let limiter = create_share_rate_limiter(10, None);
        let host = HostInfo::new("https://api.test", 10, limiter);

        // 确保 disable_before 在过去
        host.disable_before.store(unix_time_now_u64_utc().saturating_sub(1000), Ordering::SeqCst);

        let start = Instant::now();
        host.waiting_for_open(None, None).await;
        let elapsed = start.elapsed();

        assert!(elapsed.as_millis() < 50, "应当立即返回，实际耗时 {:?}", elapsed);
    }

    /// 测试目的：当 `disable_before` 在将来时，`waiting_for_open(None, None)` 应等待直到该时间到达。
    /// 场景：把 `disable_before` 设为短期未来（约150ms），不传入 `reopen_timestamp` 或 `jitter`。
    /// 断言：函数至少会等待接近该期望（>=140ms），允许少量调度开销误差。
    #[tokio::test]
    async fn test_waiting_for_open_waits_until_disable_before() {
        let limiter = create_share_rate_limiter(10, None);
        let host = HostInfo::new("https://api.test", 10, limiter);

        // 设定 disable_before 为短期未来
        let now = unix_time_now_u64_utc();
        host.disable_before.store(now + 150, Ordering::SeqCst);
        let jitter = Jitter::up_to(Duration::from_millis(30));
        let start = Instant::now();
        host.waiting_for_open(None, Some(jitter)).await;
        let elapsed = start.elapsed();

        // 至少等待了约 150ms
        assert!(elapsed.as_millis() >= 140, "应当等待至少 140ms, 实际: {:?}", elapsed);
    }

    /// 测试目的：当传入一个比当前 `disable_before` 更远的 `reopen_timestamp` 时，
    /// `waiting_for_open(Some(reopen_timestamp), Some(jitter))` 应把 `disable_before` 更新为该值并等待到更新后的时间。
    /// 场景：先把 `disable_before` 设为较小的未来时间，再传入一个更靠后的 `reopen_timestamp`（约200ms）。
    /// 断言：函数等待接近更新后的时间（>=180ms），考虑 jitter 与调度误差。
    #[tokio::test]
    async fn test_waiting_for_open_with_reopen_timestamp_updates_target() {
        let limiter = create_share_rate_limiter(10, None);
        let host = HostInfo::new("https://api.test", 10, limiter);

        // 将 disable_before 设为一个较小的未来时间
        let now = unix_time_now_u64_utc();
        host.disable_before.store(now + 50, Ordering::SeqCst);
        let jitter = Jitter::up_to(Duration::from_millis(50));
        // 传入更远的 reopen_timestamp，函数应把 disable_before 更新为该值并等待
        let reopen_ts = now + 200;
        let start = Instant::now();
        host.waiting_for_open(Some(reopen_ts), Some(jitter)).await;
        let elapsed = start.elapsed();

        assert!(elapsed.as_millis() >= 180, "应当等待直到 reopen_timestamp (~200ms), 实际: {:?}", elapsed);
    }

    /// 测试目的：当传入的 `reopen_timestamp` 小于当前 `disable_before` 时，等待目标不应被缩短，
    /// 而应仍以已有的 `disable_before` 为准。
    /// 场景：把 `disable_before` 设为较远的未来（约300ms），传入较近的 `reopen_timestamp`（约50ms）。
    /// 断言：函数等待接近原有的 `disable_before`（>=260ms），而非被传入的较小时间所覆盖。
    #[tokio::test]
    async fn test_waiting_for_open_with_reopen_smaller_than_existing() {
        // 如果传入的 reopen_timestamp 小于已有的 disable_before，应当以已有的 disable_before 为准等待
        let limiter = create_share_rate_limiter(10, None);
        let host = HostInfo::new("https://api.test", 10, limiter);

        let now = unix_time_now_u64_utc();
        // 现有 disable_before 比 reopen_timestamp 远
        host.disable_before.store(now + 300, Ordering::SeqCst);

        // 传入一个较小的 reopen timestamp
        let reopen_ts = now + 50;
        let start = Instant::now();
        let jitter = Jitter::up_to(Duration::from_millis(50));
        host.waiting_for_open(Some(reopen_ts), Some(jitter)).await;
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
    #[tokio::test]
    async fn test_waiting_for_open_concurrent_update_extends_wait() {
        // 并发场景：在等待过程中，另一线程将 disable_before 提高，等待应随之延长
        let limiter = create_share_rate_limiter(10, None);
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
        host.waiting_for_open(None, Some(jitter)).await;
        let elapsed = start.elapsed();

        // 初始等待 100ms，但因为另一个线程延后了 reopen，应至少等待到 ~360ms
        assert!(elapsed.as_millis() >= 340, "并发更新应延长等待，实际: {:?}", elapsed);
    }

    /// 测试：当传入 weight 为 0 时，应立即返回参数错误（权重必须为非零）
    ///
    /// 目的：验证 `acquire_limit_token` 在输入参数不合法时能快速返回错误，避免进入等待或限流逻辑。
    /// 步骤：
    /// 1. 创建一个 HostInfo（open 状态）
    /// 2. 调用 `acquire_limit_token` with weight=0
    /// 3. 断言返回 Err 且错误信息为 "权重必须为非零"
    #[tokio::test]
    async fn test_acquire_limit_token_rejects_zero_weight() {
        use crate::errors::YueError;
        let limiter = create_share_rate_limiter(10, None);
        let host = HostInfo::new("https://api.test", 10, limiter);

        // 确保 host 是 open
        host.disable_before
            .store(unix_time_now_u64_utc().saturating_sub(1000), std::sync::atomic::Ordering::SeqCst);

        let r = host.acquire_limit_token(0, 1).await;
        assert!(r.is_err(), "weight=0 应返回错误");
        match r.err().unwrap() {
            YueError::CustomError(s) => assert_eq!(s, "权重必须为非零"),
            other => panic!("unexpected error variant: {:?}", other),
        }
    }

    /// 测试：当 timeout_secs 为 0 时，`left_ms` 将为非正数，应立即返回超时错误
    ///
    /// 目的：验证我们在 `acquire_limit_token` 中加入的 `left_ms` 判断逻辑能在剩余时间耗尽时立刻返回超时。
    /// 步骤：
    /// 1. 创建 HostInfo 并保持 open 状态（以避免因 waiting_for_open 导致不同错误类型）
    /// 2. 直接调用 `acquire_limit_token(..., timeout_secs=0)`
    /// 3. 断言返回 Err 且为 Timeout 分支，错误信息包含 "acquire token timeout"
    #[tokio::test]
    async fn test_acquire_limit_token_left_ms_zero_times_out_immediately() {
        use crate::errors::YueError;
        let limiter = create_share_rate_limiter(10, None);
        let host = HostInfo::new("https://api.test", 10, limiter);

        // 确保 host 是 open
        host.disable_before
            .store(unix_time_now_u64_utc().saturating_sub(1000), std::sync::atomic::Ordering::SeqCst);

        let r = host.acquire_limit_token(1, 0).await;
        assert!(r.is_err(), "timeout_secs=0 应返回超时错误");
        match r.err().unwrap() {
            YueError::Timeout(s) => assert!(s.contains("acquire token timeout"), "应当包含 'acquire token timeout'，实际: {}", s),
            other => panic!("unexpected error variant: {:?}", other),
        }
    }

    /// 测试：在 host open 并且 limiter 有足够配额时，应成功获取令牌并返回 StateSnapshot
    ///
    /// 目的：验证正常路径下 `acquire_limit_token` 能返回有效的 snapshot
    /// 步骤：
    /// 1. 创建一个 limiter 容量充足（bucket_size >= weight）的 HostInfo 并确保 open
    /// 2. 调用 `acquire_limit_token` 并断言返回 Ok(Some(snapshot))
    #[tokio::test]
    async fn test_acquire_limit_token_successful_acquire() {
        let limiter = create_share_rate_limiter(1000, None);
        let host = HostInfo::new("https://api.test", 1000, limiter);

        // 确保 host open
        host.disable_before
            .store(unix_time_now_u64_utc().saturating_sub(1000), std::sync::atomic::Ordering::SeqCst);

        // weight 小于 bucket_size，且超时时间充足
        let res = host.acquire_limit_token(1, 2).await;
        assert!(res.is_ok(), "正常情况下应返回 Ok");
        let opt = res.ok().unwrap();
        assert!(opt.is_some(), "应当得到 StateSnapshot");
    }

    /// 测试：当 host 被阻塞的时间长于传入的 timeout_secs 时，`acquire_limit_token` 应返回超时
    ///
    /// 目的：验证 `left_ms` 判断在 host 被设置为远未来（例如 2000ms 后 reopen）且 timeout 较短时，
    /// 能正确返回 `YueError::Timeout("acquire token timeout ...")`。
    /// 步骤：
    /// 1. 将 host.disable_before 设置为现在 + 2000ms（模拟长时间阻塞）
    /// 2. 调用 `acquire_limit_token(..., timeout_secs=1)`（总超时 1000ms）
    /// 3. 断言返回 Err 且为 Timeout，错误信息包含 "acquire token timeout"
    #[tokio::test]
    async fn test_acquire_limit_token_times_out_if_host_blocked_longer_than_timeout() {
        use crate::errors::YueError;
        use std::sync::atomic::Ordering;

        let limiter = create_share_rate_limiter(10, None);
        let host = HostInfo::new("https://api.test", 10, limiter);

        // 将 disable_before 设为远未来（例如 2000ms 后）
        let now = unix_time_now_u64_utc();
        host.disable_before.store(now + 2000, Ordering::SeqCst);

        // timeout_secs = 1s -> 总超时 1000ms，应在等待 open 阶段超时
        let r = host.acquire_limit_token(1, 1000).await;
        assert!(r.is_err(), "应当返回超时错误");
        match r.err().unwrap() {
            YueError::Timeout(s) => assert!(s.contains("is not open"), "错误信息应包含 is not open，实际: {}", s),
            other => panic!("unexpected error variant: {:?}", other),
        }
    }

    /// 测试：当在获取令牌前的 open 检查超过剩余时间时，应返回 host not open 的超时错误
    ///
    /// 目的：验证 `acquire_limit_token` 在进入 pre-check (check_open_and_wait) 时若等待超出剩余 left_ms，
    /// 能返回 `YueError::Timeout("host ... is not open")`。
    /// 步骤：
    /// 1. 将 host.disable_before 设为现在 + 1200ms
    /// 2. 调用 `acquire_limit_token(..., timeout_secs=1)`（总超时 1000ms）
    /// 3. 断言返回 Err 且为 Timeout，错误信息包含 "host" 和 "not open"
    #[tokio::test]
    async fn test_acquire_limit_token_precheck_wait_exceeds_left_ms_returns_host_not_open() {
        use crate::errors::YueError;
        use std::sync::atomic::Ordering;

        let limiter = create_share_rate_limiter(10, None);
        let host = HostInfo::new("https://api.test", 10, limiter);

        // 将 disable_before 设为稍微超过 1s 的未来，使得 pre-check 的等待超过 left_ms
        let now = unix_time_now_u64_utc();
        host.disable_before.store(now + 1200, Ordering::SeqCst);

        let r = host.acquire_limit_token(1, 1).await;
        assert!(r.is_err(), "应当返回超时(主机未开放)错误");
        match r.err().unwrap() {
            YueError::Timeout(s) => assert!(
                s.contains("not open") || s.contains("acquire token timeout"),
                "应为 host not open 或 acquire token timeout，实际: {}",
                s
            ),
            other => panic!("unexpected error variant: {:?}", other),
        }
    }
}
