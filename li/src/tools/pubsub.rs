use actix::{Message as ActixMessage, Recipient};

/// 通用订阅事件消息，用于订阅任何类型的事件
/// 泛型 E 表示要订阅的事件类型，必须实现 ActixMessage
/// 这将原本的 SubscribeTask 抽象为更通用的形式，供其他订阅模式使用
#[derive(Debug, Clone)]
pub struct SubscribeEvent<E: ActixMessage + Send>(pub Recipient<E>)
where
    E: Send,
    <E as ActixMessage>::Result: Send;

impl<E: ActixMessage + Send> ActixMessage for SubscribeEvent<E>
where
    E: Send,
    <E as ActixMessage>::Result: Send,
{
    type Result = ();
}

/// 宏：简化订阅事件的逻辑
/// 用法：subscribe_event!(publisher_addr, recipient, EventType);
/// recipient 是 Recipient<EventType>，这会自动发送 SubscribeEvent<EventType> 消息，注册订阅者。
/// 示例：subscribe_event!(cron_addr, subscriber.recipient(), TaskCompletionEvent);
#[macro_export]
macro_rules! subscribe_event {
    ($publisher_addr:expr, $recipient:expr, $event_type:ty) => {
        $publisher_addr.do_send($crate::tools::SubscribeEvent::<$event_type>($recipient));
    };
}

/// 宏：使用 addr 订阅事件
/// 用法：subscribe_event_addr!(publisher_addr, subscriber_addr, EventType);
/// subscriber_addr 是 Addr，宏内部会调用 .recipient()，这会自动发送 SubscribeEvent<EventType> 消息，注册订阅者。
/// 示例：subscribe_event_addr!(cron_addr, subscriber, TaskCompletionEvent);
#[macro_export]
macro_rules! subscribe_event_addr {
    ($publisher_addr:expr, $subscriber_addr:expr, $event_type:ty) => {
        $publisher_addr.do_send($crate::tools::SubscribeEvent::<$event_type>($subscriber_addr.recipient()));
    };
}
