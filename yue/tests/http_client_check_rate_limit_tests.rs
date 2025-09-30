#[cfg(test)]
mod tests {
    use yue::http_client::{DefaultRateLimiter, check_rate_limit};

    fn get_test_rate_limiter(burst: u32) -> DefaultRateLimiter {
        use governor::{Quota, RateLimiter};
        use std::num::NonZeroU32;
        let burst = NonZeroU32::new(burst).unwrap();
        let quota = Quota::per_minute(burst);
        RateLimiter::direct(quota)
    }

    #[tokio::test]
    async fn test_rate_limited() {
        let limiter = get_test_rate_limiter(1200);
        // 测试正常调��
        let result = check_rate_limit(1, &limiter, 2).await;
        assert!(result.is_ok());

        // 测试高权重调用，触发超时
        let result = check_rate_limit(1201, &limiter, 2).await; // 超过突发容量
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_zero_weight() {
        let limiter = get_test_rate_limiter(1200);
        // 测试零权重，预期错误
        let result = check_rate_limit(0, &limiter, 2).await;
        assert!(result.is_err());
    }
}
