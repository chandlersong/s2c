//主要是提供一些不断循环的宏

use log::info;
use tokio::sync::OnceCell;

///
/// 不停的调用各种异步运行的方法
///
///
use tokio::sync::broadcast;


static STOP_NOTIFICATION: OnceCell<broadcast::Sender<()>> = OnceCell::const_new();


async fn initial_stop_notification() -> broadcast::Sender<()> {
    let (tx, _) = broadcast::channel(1000);
    tx
}

pub async fn endless_stop_tx() -> broadcast::Sender<()> {
    STOP_NOTIFICATION.get_or_init(initial_stop_notification).await.clone()
}


pub async fn stop_endless() {
    let tx = endless_stop_tx().await;
    info!("stopped the endless program");
    tx.send(()).unwrap();
}
#[macro_export]
macro_rules! async_endless {
       ( $start_at:expr,             //循环开始时间
         $sleep_duration:expr,         //循环周期
         async {$($action:tt)*}       // 循环中做的事情
         $(,async {$($stop_action:tt)* })?  //结束的行为
       ) => {
         tokio::spawn(
             async move {
                 use tokio::signal;
                 let mut start = $start_at.clone();
                 let mut rx = endless_stop_tx().await.subscribe();
                 loop {
                    tokio::select! {
                             _ = signal::ctrl_c() => {
                                 $(
                                   (async { $($stop_action)* }).await;
                                 )?
                                 break;
                             }
                             _ = rx.recv() => {
                                 $(
                                   (async { $($stop_action)* }).await;
                                 )?
                                 break;
                             }
                             _ = sleep_until(start) => {
                                    (async { $($action)* }).await
                             }
                    }
                    start = start + $sleep_duration;
                };
             }
         )
    };
}

///
/// 启动一个新的新的县城。然后不停的处理
#[macro_export]
macro_rules! endless_select {
    (
      $pat:pat = $fut:expr => $act:block //select执行分支
       $(,async {$($stop_action:tt)* })?  //结束的行为
     ) => {
        tokio::spawn(
            async move {
                use tokio::signal;
                let mut stop_rx = endless_stop_tx().await.subscribe();
                loop{
                    tokio::select! {
                        $pat = $fut => $act
                         _ = signal::ctrl_c() => {
                                 $(
                                   (async { $($stop_action)* }).await;
                                 )?
                                 break;
                         }
                         _ = stop_rx.recv() => {
                             $(
                               (async { $($stop_action)* }).await;
                             )?
                             break;
                         }
                    }
                }
            }
        )
    };
}


#[cfg(test)]
pub mod tests {
    use crate::tools::endless::{endless_stop_tx, stop_endless};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::signal;
    use tokio::sync::broadcast;
    use tokio::time::{sleep, sleep_until, Instant};

    ///
    /// 因为这个测试是不间断的跑。所以不用没法一直测试。
    #[ignore]
    #[tokio::test]
    async fn test_loop_marco() {
        let mut a = 1;
        let _ = async_endless! {
            Instant::now() + Duration::from_millis(10),
            Duration::from_millis(100),
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
        signal::ctrl_c().await.expect("TODO: panic message");
    }


    #[tokio::test]
    async fn test_manually_stop() {
        let value = Arc::new(Mutex::new(1));
        let clone = value.clone();
        let _ = async_endless! {
            Instant::now() + Duration::from_millis(10),
            Duration::from_millis(50),
            async {
               *clone.lock().unwrap() += 1;
            }
        };

        sleep(Duration::from_millis(200)).await;
        stop_endless().await;
        let guard = value.lock().unwrap();
        assert_eq!(*guard, 5);
    }

    #[tokio::test]
    async fn test_endless_select() {
        let value = Arc::new(Mutex::new(1));
        let clone = value.clone();
        let (tx, mut rx) = broadcast::channel(1);
        let _ = endless_select!(
                num = rx.recv() => {
                   *clone.lock().unwrap() = num.unwrap();
                }
        );
        tx.send(8).unwrap();
        sleep(Duration::from_millis(200)).await;
        stop_endless().await;
        println!("Hello, world! {}", value.lock().unwrap());
    }
}
