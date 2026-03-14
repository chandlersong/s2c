#[cfg(test)]
mod tests {
    use futures_util::future::join_all;
    use governor::{Quota, RateLimiter};
    use nonzero::nonzero;
    use std::num::NonZeroU32;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::task;
    use yue::http_client::check_rate_limit;
    use yue::models::DefaultRateLimiter;

    fn get_test_rate_limiter(burst: u32) -> DefaultRateLimiter {
        let burst = NonZeroU32::new(burst).unwrap();
        // 设置速率为每小时 burst 次，突发桶容量为 burst（允许瞬间通过 burst 个请求）
        let quota = Quota::per_hour(burst).allow_burst(burst);
        RateLimiter::direct(quota)
    }

    #[tokio::test]
    async fn test_rate_limited() {
        let limiter = get_test_rate_limiter(1200);
        // 测试正常调��
        let result = check_rate_limit(1, &limiter, 1).await;
        assert!(result.is_ok());

        // 测试高权重调用，触发超时
        let result = check_rate_limit(1201, &limiter, 1).await; // 超过突发容量
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_zero_weight() {
        let limiter = get_test_rate_limiter(1200);
        // 测试零权重，预期错误
        let result = check_rate_limit(0, &limiter, 1).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_concurrent_rate_limit() {
        use std::sync::Arc;
        use tokio::task;
        let limiter = Arc::new(get_test_rate_limiter(5)); // 每秒最多5次
        let mut handles = Vec::new();
        for _ in 0..5 {
            let limiter_ref = Arc::clone(&limiter);
            handles.push(task::spawn(async move { check_rate_limit(1, &limiter_ref, 1).await }));
        }
        // 等待所有任务完成
        let results = join_all(handles).await;
        for res in results {
            assert!(res.unwrap().is_ok());
        }
    }

    #[tokio::test]
    async fn test_concurrent_rate_limit_with_timeout() {
        let limiter = Arc::new(get_test_rate_limiter(5)); // 每小时最多5次，允许瞬间通过5个
        let mut handles = Vec::new();
        for _ in 0..10 {
            let limiter_ref = limiter.clone();
            handles.push(task::spawn(async move { check_rate_limit(1, &limiter_ref, 1).await }));
        }
        // 等待所有任务完成
        let results = join_all(handles).await;
        assert_eq!(results.len(), 10);
        let mut success_count = 0;
        let mut fail_count = 0;
        for res in results {
            let res = res.unwrap(); // task join
            if res.is_ok() {
                success_count += 1;
            } else {
                fail_count += 1;
            }
        }
        assert_eq!(success_count, 5, "成功任务数应为5，实际为{}", success_count);
        assert_eq!(fail_count, 5, "失败任务数应为5，实际为{}", fail_count);
    }

    #[tokio::test]
    async fn test_concurrent_rate_limit_full_fill() {
        let quota = Quota::with_period(Duration::from_millis(200)).unwrap().allow_burst(nonzero!(5u32)); // 每200毫秒补充1个，突发容量5));
        let limiter = Arc::new(RateLimiter::direct(quota)); // 每小时最多5次，允许瞬间通过5个
        let mut handles = Vec::new();
        for _ in 0..10 {
            let limiter_ref = limiter.clone();
            handles.push(task::spawn(async move { check_rate_limit(1, &limiter_ref, 2).await }));
        }
        // clock.advance(Duration::from_secs(300));
        // 等待所有任务完成
        let results = join_all(handles).await;
        assert_eq!(results.len(), 10);
        let mut success_num = 0;
        for res in results {
            let res = res.unwrap(); // task join
            assert!(res.is_ok(), "任务应该成功,已经成功的次数：{}", success_num);
            success_num = success_num + 1;
        }
    }
}
