/** 这个mod的主要作用是为了统一的接口
当然，因为历史原因，主要是我不习惯rust的写法，所以第一版本的api弄的很差。所以才有了这样一段心累。故定义下列规则
    1. 这里封装所有的网络调用。
       - restful返回oneshot:tx
       - websocket返回stream
    2. 所有都以宏带来放入。
    3. 一些公用功能写在宏中。
**/





// #[macro_export]
// /// 限流宏，接受权重和代码块
// macro_rules! rate_limited {
//     ($weight:expr, $block:expr) => {{
//         use std::num::NonZeroU32;
//         async {
//             // 获取 RateLimiter
//             let limiter = get_rate_limiter();
//             // 超时时间：2 秒
//             let timeout_duration = Duration::from_secs(2);
//             // 抖动避免请求堆积
//             let jitter = Jitter::up_to(Duration::from_millis(100));
//
//             // 验证权重非零
//             let weight = match NonZeroU32::new($weight) {
//                 Some(w) => w,
//                 None => return Err("Weight must be non-zero".to_string()),
//             };
//             // 等待令牌或超时
//             let result = timeout(
//                 timeout_duration,
//                 limiter.until_n_ready_with_jitter(weight, jitter),
//             ).await;
//          match result {
//                 Ok(inner_result) => match inner_result {
//                     Ok(()) => Ok($block),
//                     Err(_) => Err("令牌不足".to_string()),
//                 },
//                 Err(_) => Err("限流超时".to_string()),
//             }
//         }
//     }};
// }

#[cfg(test)]
mod tests {

    // #[tokio::test]
    // async fn test_rate_limited() {
    //     // 测试正常调用
    //     let result = rate_limited!(1, 42).await;
    //     assert!(result.is_ok());
    //     assert_eq!(result.unwrap(), 42);
    //
    //     // 测试高权重调用，触发超时
    //     let result = rate_limited!(30, 42).await; // 超过突发容量
    //     assert!(result.is_err());
    // }
    //
    // #[tokio::test]
    // async fn test_zero_weight() {
    //     // 测试零权重，预期错误
    //     let result = rate_limited!(0, 42).await;
    //     assert!(result.is_err());
    //     assert_eq!(result.unwrap_err(), "Weight must be non-zero");
    // }
}
