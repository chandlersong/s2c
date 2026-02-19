use crate::binance::bn_models::common::ListenKeyResponse;
use crate::binance::bn_restful_commands::{BNSecurityRequestBuilder, SWAP_LISTEN_KEY_COMMAND, execute_bn_post, execute_bn_put};
use crate::binance::history_data::CommonParam;
use crate::errors::YueError;
use crate::models::RequestInfo;
use actix::{Actor, AsyncContext, Context, Handler, Message as ActixMessage};
use log::{debug, error, info};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::sleep;

/// 手动续期 listen key
#[derive(Clone)]
pub struct RenewListenKey;

impl ActixMessage for RenewListenKey {
    type Result = Result<(), YueError>;
}

///
/// # 说明
/// 根据币安的说明。[币本位账户信息流](https://developers.binance.com/docs/zh-CN/derivatives/coin-margined-futures/user-data-streams)
/// 在通过websocket监听swap等account变化的时候。大致步骤是
/// 1. 通过REST接口获取listen key
/// 2. 通过websocket，带上listen key去监听。
///
/// 而对于listen key的维护，币安的说明是：
/// 1. listen key的有效期为60分钟，过期后需要延长或者更新listen key
/// 2. 单个连接只能有一个listen key，如果需要监听多个账户或者多个交易所，需要建立多个连接。
/// 3. 单个链接，24小时后会断开重连。
///

///
/// # 设计思路
/// 1. 一个client，对应一个用户的websocket的定义方式。
/// 2. 这个client的职责是维护listen key的状态，定时更新listen key，提供接口获取listen key。
/// 3. 维持websocket的逻辑，可以参考WebSocketClient中维护websocket连接的逻辑，定时器等。
///
/// # 扩展点
/// - 可以加入失败重试机制和指数退避
/// - 可以加入 listen key 过期前提前续期的策略
/// - 可以支持多个订阅者监听 listen key 变化事件
///
#[derive(Clone)]
pub struct ListenKeyClient {
    // 当前的 listen key，使用 Arc<Mutex<>> 保证线程安全
    listen_key: Arc<Mutex<String>>,
    // 获取新 listen key 的请求信息
    pub apply_listen_key_request: RequestInfo,
    // 续期 listen key 的请求信息
    pub renew_listen_key_request: RequestInfo,
    // 续期间隔（毫秒），默认 3600000ms = 60 分钟
    pub renew_interval_ms: u64,
    api_key: String,
    api_secret: String,
    name: String,
}

impl ListenKeyClient {
    /// 创建新的 ListenKeyClient
    pub fn new(
        name: &str,
        apply_listen_key_request: RequestInfo,
        renew_listen_key_request: RequestInfo,
        renew_interval_ms: Option<u64>,
        api_key: &str,
        api_secret: &str,
    ) -> Self {
        let actual_renew_interval_ms = renew_interval_ms.unwrap_or(55 * 60 * 1000); // 默认 55 分钟
        Self {
            listen_key: Arc::new(Mutex::new(String::new())),
            apply_listen_key_request,
            renew_listen_key_request,
            renew_interval_ms: actual_renew_interval_ms,
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
            name: name.to_string(),
        }
    }

    /// 内部方法：获取当前的 listen key
    fn get_current_listen_key(&self) -> String {
        self.listen_key.lock().unwrap().clone()
    }

    /// 内部方法：设置新的 listen key
    #[allow(dead_code)]
    fn set_listen_key(&self, key: String) {
        *self.listen_key.lock().unwrap() = key;
    }

    /// 发送 HTTP 请求获取新的 listen key
    #[allow(dead_code)]
    async fn fetch_new_listen_key(&self) -> Result<String, YueError> {
        info!("正在获取新的 listen key...");

        let builder = BNSecurityRequestBuilder::new(self.api_key.to_string(), self.api_secret.to_string());
        let create_response =
            execute_bn_post::<CommonParam, BNSecurityRequestBuilder, ListenKeyResponse>(&self.apply_listen_key_request, None, None, builder)
                .execute()
                .await?;
        info!(
            "成功获取 listen key: {}",
            create_response.listen_key.chars().take(20).collect::<String>() + "..."
        );
        Ok(create_response.listen_key)
    }

    /// 发送 HTTP 请求续期 listen key
    async fn renew_current_listen_key(&self) -> Result<(), YueError> {
        let builder = BNSecurityRequestBuilder::new(self.api_key.to_string(), self.api_secret.to_string());
        let renew_response =
            execute_bn_put::<CommonParam, BNSecurityRequestBuilder, ListenKeyResponse>(&SWAP_LISTEN_KEY_COMMAND, None, None, builder)
                .execute()
                .await?;
        println!("renew listen_key is {:?}", renew_response.listen_key);

        info!("成功续期 listen key");
        Ok(())
    }
}

impl Actor for ListenKeyClient {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("ListenKeyClient started, 续期间隔: {} ms", self.renew_interval_ms);

        // 启动定时续期任务
        let addr = ctx.address();
        let interval_duration = Duration::from_millis(self.renew_interval_ms);

        let listen_key = self.get_current_listen_key();
        info!("开始监听账户,账户为: {} ", self.name);

        tokio::spawn(async move {
            loop {
                sleep(interval_duration).await;
                match addr.try_send(RenewListenKey) {
                    Ok(_) => {
                        debug!("续期任务已发送");
                    }
                    Err(e) => {
                        error!("发送续期任务失败: {}", e);
                    }
                }
            }
        });
    }
}

impl Handler<RenewListenKey> for ListenKeyClient {
    type Result = actix::ResponseActFuture<Self, Result<(), YueError>>;

    fn handle(&mut self, _msg: RenewListenKey, _ctx: &mut Context<Self>) -> Self::Result {
        let client_self = self.clone();

        let fut = async move { client_self.renew_current_listen_key().await };
        Box::pin(actix::fut::wrap_future::<_, Self>(fut))
    }
}
