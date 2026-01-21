use crate::errors::YueError;
use crate::websocket::client::{WebSocketClient, WebSocketEvent};
use actix::{Actor, Addr, Context, Handler, Message as ActixMessage, Recipient, Supervised};
use log::{error, info, trace, warn};
use std::fmt::Debug;

/// Parser trait 定义（内置于 WsMessageBus 的泛型）
pub trait WebSocketHandler: Send + Sync + Unpin + 'static {
    /// 解析输出类型
    type Output: ActixMessage<Result = ()> + Clone + Send + Debug + 'static;

    /// 解析文本消息
    fn parse_text(&self, _text: &str) -> Result<Self::Output, YueError> {
        Err(YueError::NotImplemented("text parsing not implemented".to_string()))
    }

    /// 解析二进制消息（暂未实现，占位返回 NotImplemented）
    fn parse_binary(&self, _data: &[u8]) -> Result<Self::Output, YueError> {
        Err(YueError::NotImplemented("binary parsing not implemented".to_string()))
    }

    /// 连接建立时的回调（可选实现）
    /// 这里传入一个addr用来发消息，我觉得有点简单。想要包装一个类。但是觉得也太复杂。就先这样吧。
    fn on_connect(&self, _addr: &Addr<WebSocketClient>) -> Result<(), YueError> {
        Ok(())
    }
}

/// 订阅消息
#[derive(Debug, Clone)]
pub struct Subscribe<P: WebSocketHandler> {
    pub subscriber: Recipient<P::Output>,
}

impl<P: WebSocketHandler> ActixMessage for Subscribe<P> {
    type Result = Result<(), YueError>;
}

/// WebSocket 消息总线
/// 订阅 WebSocketClient 的 WebSocketEvent，解析后广播到多个订阅者
pub struct WsMessageBus<P: WebSocketHandler> {
    handler: P,
    subscribers: Vec<Recipient<P::Output>>,
    // 统计信息（仅用于日志，无需通过消息返回，先统计，不做任何输出
    text_messages_received: u64,
    binary_messages_received: u64,
    parse_failed: u64,
    broadcast_count: u64,
    // 发送失败统计
    send_failed_count: u64,
    // 上次警告时间（用于限流日志）
    last_warning_time: std::time::Instant,
}

impl<P: WebSocketHandler> WsMessageBus<P> {
    pub fn new(handler: P) -> Self {
        Self {
            handler,
            subscribers: Vec::new(),
            text_messages_received: 0,
            binary_messages_received: 0,
            parse_failed: 0,
            broadcast_count: 0,
            send_failed_count: 0,
            last_warning_time: std::time::Instant::now(),
        }
    }

    /// 广播消息到所有订阅者
    ///
    /// 策略：
    /// 1. 优先使用 try_send（非阻塞，快速失败）
    /// 2. 如果失败，使用 do_send（会自动扩展邮箱，保证送达）
    /// 3. 统计失败次数，定期输出警告日志
    fn broadcast(&mut self, output: P::Output) {
        for subscriber in &self.subscribers {
            if let Err(e) = subscriber.try_send(output.clone()) {
                // try_send 失败，降级使用 do_send
                self.send_failed_count += 1;

                // 使用 do_send 保证消息送达（会自动扩展邮箱）
                subscriber.do_send(output.clone());

                // 限流警告日志：每10秒最多输出一次
                let now = std::time::Instant::now();
                if now.duration_since(self.last_warning_time).as_secs() >= 10 {
                    warn!(
                        "⚠️ 订阅者邮箱满，已使用 do_send 保证送达。累计失败 {} 次。错误: {}",
                        self.send_failed_count, e
                    );
                    warn!("💡 建议：1) 增加订阅者处理速度 2) 增加邮箱容量 3) 减少批量大小");
                    self.last_warning_time = now;
                }
            }
        }
    }
}

impl<P: WebSocketHandler> Actor for WsMessageBus<P> {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("WsMessageBus started");
    }
}

impl<P: WebSocketHandler> Supervised for WsMessageBus<P> {
    fn restarting(&mut self, _ctx: &mut Context<Self>) {
        info!("WsMessageBus restarting by supervisor");
    }
}

/// 处理 Subscribe 消息
impl<P: WebSocketHandler> Handler<Subscribe<P>> for WsMessageBus<P> {
    type Result = Result<(), YueError>;

    fn handle(&mut self, msg: Subscribe<P>, _ctx: &mut Context<Self>) -> Self::Result {
        self.subscribers.push(msg.subscriber);
        info!("Subscriber registered. Total subscribers: {}", self.subscribers.len());
        Ok(())
    }
}
impl<P: WebSocketHandler> Handler<WebSocketEvent> for WsMessageBus<P> {
    type Result = ();

    fn handle(&mut self, event: WebSocketEvent, _ctx: &mut Context<Self>) {
        match event {
            WebSocketEvent::Connected(_addr) => {
                info!("✓ WebSocket 已连接");
                if let Err(e) = self.handler.on_connect(&_addr) {
                    error!("启动的业务逻辑失败。{}", e);
                }
            }
            WebSocketEvent::TextMessage(text) => {
                self.text_messages_received += 1;
                trace!("收到文本消息 #{}: {} 字符", self.text_messages_received, text.len());
                match self.handler.parse_text(text.as_str()) {
                    Ok(m) => {
                        self.broadcast(m);
                        self.broadcast_count += 1;
                        trace!("✓ 解析并广播消息 #{}", self.broadcast_count);
                    }
                    Err(e) => {
                        self.parse_failed += 1;
                        error!(
                            "❌ 解析文本消息失败 (失败 #{}/{}): {}\n原始消息: {}",
                            self.parse_failed, self.text_messages_received, e, text
                        );
                    }
                }
            }
            WebSocketEvent::BinaryMessage(data) => {
                self.binary_messages_received += 1;
                trace!("收到二进制消息 #{}: {} 字节", self.binary_messages_received, data.len());
                match self.handler.parse_binary(&data) {
                    Ok(m) => {
                        self.broadcast(m);
                        self.broadcast_count += 1;
                        trace!("✓ 解析并广播消息 #{}", self.broadcast_count);
                    }
                    Err(e) => {
                        self.parse_failed += 1;
                        error!(
                            "❌ 解析二进制消息失败 (失败 #{}/{}): {}\n数据长度: {} 字节",
                            self.parse_failed,
                            self.binary_messages_received,
                            e,
                            data.len()
                        );
                    }
                }
            }
            WebSocketEvent::Reconnecting => {
                info!("🔄 WebSocket 正在重新连接...");
            }
            WebSocketEvent::Disconnected => {
                info!(
                    "✗ WebSocket 已断开 - 统计: 文本 {}, 二进制 {}, 广播 {}, 解析失败 {}, 发送失败 {}",
                    self.text_messages_received, self.binary_messages_received, self.broadcast_count, self.parse_failed, self.send_failed_count
                );
            }
            WebSocketEvent::Error(err) => {
                error!("❌ WebSocket 错误: {}", err);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// 简单的测试 Parser
    struct TestParser;

    #[derive(Debug, Clone, PartialEq)]
    struct TestOutput {
        content: String,
    }

    impl ActixMessage for TestOutput {
        type Result = ();
    }

    impl WebSocketHandler for TestParser {
        type Output = TestOutput;

        fn parse_text(&self, text: &str) -> Result<Self::Output, YueError> {
            if text.is_empty() {
                Err(YueError::ParseError("empty text".to_string()))
            } else {
                Ok(TestOutput { content: text.to_string() })
            }
        }
    }

    /// 带二进制解析支持的 Parser
    struct BinaryParser;

    #[derive(Debug, Clone, PartialEq)]
    struct BinaryOutput {
        data: Vec<u8>,
    }

    impl ActixMessage for BinaryOutput {
        type Result = ();
    }

    impl WebSocketHandler for BinaryParser {
        type Output = BinaryOutput;

        fn parse_text(&self, text: &str) -> Result<Self::Output, YueError> {
            Ok(BinaryOutput {
                data: text.as_bytes().to_vec(),
            })
        }

        fn parse_binary(&self, data: &[u8]) -> Result<Self::Output, YueError> {
            if data.is_empty() {
                Err(YueError::ParseError("empty binary data".to_string()))
            } else {
                Ok(BinaryOutput { data: data.to_vec() })
            }
        }
    }

    #[test]
    fn test_ws_message_bus_creation() {
        let parser = TestParser;
        let bus = WsMessageBus::new(parser);
        assert_eq!(bus.subscribers.len(), 0);
        assert_eq!(bus.text_messages_received, 0);
        assert_eq!(bus.binary_messages_received, 0);
        assert_eq!(bus.parse_failed, 0);
        assert_eq!(bus.broadcast_count, 0);
    }

    #[test]
    fn test_parse_text() {
        let parser = TestParser;
        let result = parser.parse_text("hello");
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.content, "hello");
    }

    #[test]
    fn test_parse_text_empty() {
        let parser = TestParser;
        let result = parser.parse_text("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_binary_not_implemented() {
        let parser = TestParser;
        let result = parser.parse_binary(&[1, 2, 3]);
        assert!(result.is_err());
        match result {
            Err(YueError::NotImplemented(_)) => (),
            _ => panic!("Expected NotImplemented error"),
        }
    }

    #[test]
    fn test_binary_parser_parse_text() {
        let parser = BinaryParser;
        let result = parser.parse_text("test");
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.data, b"test");
    }

    #[test]
    fn test_binary_parser_parse_binary() {
        let parser = BinaryParser;
        let data = vec![1, 2, 3, 4, 5];
        let result = parser.parse_binary(&data);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.data, data);
    }

    #[test]
    fn test_binary_parser_parse_empty_binary() {
        let parser = BinaryParser;
        let result = parser.parse_binary(&[]);
        assert!(result.is_err());
        match result {
            Err(YueError::ParseError(_)) => (),
            _ => panic!("Expected ParseError"),
        }
    }

    #[test]
    fn test_ws_message_bus_broadcast() {
        let parser = TestParser;
        let mut bus = WsMessageBus::new(parser);

        let output = TestOutput { content: "test".to_string() };

        // 测试空订阅者列表的广播（不应该崩溃）
        bus.broadcast(output);
    }

    #[test]
    fn test_parser_parse_multiple_messages() {
        let parser = TestParser;

        // 测试多条消息解析
        let messages = vec!["msg1", "msg2", "msg3"];
        for msg in messages {
            let result = parser.parse_text(msg);
            assert!(result.is_ok());
            assert_eq!(result.unwrap().content, msg);
        }
    }

    #[test]
    fn test_binary_parser_various_data() {
        let parser = BinaryParser;

        // 测试不同大小的二进制数据
        let test_cases = vec![vec![1], vec![1, 2, 3], vec![255, 0, 128], (0..100).collect::<Vec<u8>>()];

        for data in test_cases {
            let result = parser.parse_binary(&data);
            assert!(result.is_ok());
            assert_eq!(result.unwrap().data, data);
        }
    }
}
