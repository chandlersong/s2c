//主要是提供一些不断循环的宏

///
/// 不停的调用各种异步运行的方法
///
///

#[macro_export]
macro_rules! async_endless {
       ( $start_at:expr,             //循环开始时间
         $sleep_seconds:expr,         //循环周期
         async {$($action:tt)*}       // 循环中做的事情
         $(,async {$($ctrl_c_stop:tt)* })?  //ctrl+c结束做的事情
       ) => {
         use tokio::signal;
         let mut start = $start_at.clone();
         loop {
            tokio::select! {
                     _ = signal::ctrl_c() => {
                         $(
                           (async { $($ctrl_c_stop)* }).await;
                         )?
                         break;
                     }
                     _ = sleep_until(start) => {
                            (async { $($action)* }).await
                     }
            }
            start = start + Duration::from_secs($sleep_seconds as u64);
        };
    };
}

#[cfg(test)]
pub mod tests {
    use std::time::Duration;
    use tokio::time::{sleep_until, Instant};
    ///
    /// 因为这个测试是不间断的跑。所以不用没法一直测试。
    #[ignore]
    #[tokio::test]
    async fn test_loop_marco() {
        let mut a = 1;
        async_endless! {
           Instant::now() + Duration::from_secs(1), 1,
            async {
                println!("Hi,{}",a);
                a= a+1;
            },
            async {
                println!("stop,{}",a);
                a= a+1;
            }
        }
        println!("Hello, world!{}", a);
    }
}
