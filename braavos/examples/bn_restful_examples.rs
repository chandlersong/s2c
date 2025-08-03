use braavos::binance::bn_restful_commands::execute_ping;
use braavos::http_client::init_http_client;

///
/// 这个example的主要作用是
/// 1. 展示如果去

#[tokio::main]
async fn main() {
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy);
    let _ = execute_ping();


    // let get = GetCommand::<EmptyObject, EmptyObject> { phantom: Default::default() };
    // let x = get.execute(info, None, None).await.unwrap();
    // assert_eq!(x, EmptyObject {})


    // let result = rate_limited!(1, 42).await;
    // assert!(result.is_ok());
    // assert_eq!(result.unwrap(), 42);
    //
    // // 测试高权重调用，触发超时
    // let result = rate_limited!(30, 42).await; // 超过突发容量
    // assert!(result.is_err());
}
