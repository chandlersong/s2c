//主要是提供一些不断循环的宏

///
/// 不停的调用各种异步运行的方法
///
///

#[macro_export]
macro_rules! async_endless {
    (async { $($action:tt)* } $(,async {$($ctrl_c_stop:tt)* })? ) => {
         use tokio::time::{sleep_until, Instant};
         use tokio::signal;
         use std::time::Duration;
         let mut start = Instant::now() + Duration::from_secs(1);
         loop {
            let sleep_seconds = 1;
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
            start = start + Duration::from_secs(sleep_seconds as u64);
        };
    };
}

#[cfg(test)]
pub mod tests {
    ///
    /// 因为这个测试是不间断的跑。所以不用没法一直测试。
    #[ignore]
    #[tokio::test]
    async fn test_loop_marco() {
        let mut a = 1;
        async_endless! {
            async {
                println!("Hi,{}",a);
                a= a+1;
            },
            async {
                println!("stop,{}",a);
                a= a+1;
            }
        };
        println!("Hello, world!{}", a);
    }
}
