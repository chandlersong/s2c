use crate::models::{DefaultRateLimiter, HostInfo, RequestInfo};
use governor::middleware::StateInformationMiddleware;
use governor::{Quota, RateLimiter};
use std::num::NonZeroU32;
use std::sync::{Arc, LazyLock};
use tokio::sync::RwLock;

///
/// 因为根据OKX的规则。每个api都有自己的ratelimit。
/// 这个其实和之前不一样。
/// 所以，这里用最低的来做2s，10个
///
pub static BURST_NUM: u32 = 5;
pub static SPOT_RATE_PER_MINUTE: u32 = 5;
fn create_default_rate_limiter(bucket_size: u32, burst_size: Option<u32>) -> Arc<RwLock<Arc<DefaultRateLimiter>>> {
    let real_burst_size = burst_size.unwrap_or(bucket_size);
    // Interpret `bucket_size` as tokens per second. For example, bucket_size=10 -> 10 tokens/sec -> 20 tokens/2s
    let quota = Quota::per_second(NonZeroU32::new(bucket_size).unwrap()).allow_burst(NonZeroU32::new(real_burst_size).unwrap().into());

    let res = RateLimiter::direct(quota);
    let limiter_with_info = res.with_middleware::<StateInformationMiddleware>();
    Arc::new(RwLock::new(Arc::new(limiter_with_info)))
}

pub const OKX_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "https://openapi.okx.com",
        SPOT_RATE_PER_MINUTE,
        create_default_rate_limiter(BURST_NUM, Some(SPOT_RATE_PER_MINUTE)),
    ))
});

pub const PUBLIC_INSTRUMENTS: &str = "/api/v5/public/instruments";
pub const HISTORY_CANDLES: &str = "/api/v5/market/history-candles";

pub const CANDLES: &str = "/api/v5/market/candles";
pub static PUBLIC_INSTRUMENTS_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(OKX_BASE.clone(), PUBLIC_INSTRUMENTS, false, 1, None, None).unwrap());

pub static HISTORY_CANDLES_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(OKX_BASE.clone(), HISTORY_CANDLES, false, 1, None, None).unwrap());
