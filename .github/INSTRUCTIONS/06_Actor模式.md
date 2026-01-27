# Actor 模式使用指南

## Actix Actor 基础

### 什么是 Actor

Actor 是独立的、能接收和发送消息的计算单元。Actix 框架提供高性能的 Actor 实现。

特点：
- **隔离**：每个 Actor 有独立的状态和邮箱
- **异步**：通过消息传递，不共享内存
- **可扩展**：支持远程和集群部署
- **容错**：Actor 崩溃时可以自动重启

## 定义一个简单 Actor

### 最小示例

```rust
use actix::{Actor, Context, Handler, Message as ActixMessage};

// 定义消息
#[derive(Debug, Clone)]
pub struct MyMessage {
    pub data: String,
}

impl ActixMessage for MyMessage {
    type Result = ();  // 消息返回类型（这里无返回值）
}

// 定义 Actor
pub struct MyActor {
    name: String,
    counter: u32,
}

impl Actor for MyActor {
    type Context = Context<Self>;
    
    fn started(&mut self, _ctx: &mut Self::Context) {
        println!("{} started", self.name);
    }
    
    fn stopped(&mut self, _ctx: &mut Self::Context) {
        println!("{} stopped", self.name);
    }
}

// 实现消息处理器
impl Handler<MyMessage> for MyActor {
    type Result = ();
    
    fn handle(&mut self, msg: MyMessage, _ctx: &mut Context<Self>) {
        self.counter += 1;
        println!("{} received: {} (count: {})", self.name, msg.data, self.counter);
    }
}

// 使用
#[actix::main]
async fn main() {
    let actor = MyActor {
        name: "MyActor".to_string(),
        counter: 0,
    }.start();
    
    // 发送消息（异步，无等待）
    actor.do_send(MyMessage {
        data: "Hello".to_string(),
    });
    
    tokio::time::sleep(Duration::from_secs(1)).await;
}
```

## 消息类型和返回值

### 无返回值（fire-and-forget）

```rust
#[derive(Debug)]
pub struct Log {
    pub message: String,
}

impl ActixMessage for Log {
    type Result = ();  // 无返回值
}

impl Handler<Log> for LoggerActor {
    type Result = ();
    
    fn handle(&mut self, msg: Log, _ctx: &mut Context<Self>) {
        println!("{}", msg.message);
    }
}

// 使用
actor.do_send(Log { message: "Info".to_string() });  // 不等待
```

### 有返回值（request-reply）

```rust
#[derive(Debug)]
pub struct GetCount;

impl ActixMessage for GetCount {
    type Result = u32;  // 返回 u32
}

impl Handler<GetCount> for CounterActor {
    type Result = u32;
    
    fn handle(&mut self, _msg: GetCount, _ctx: &mut Context<Self>) -> u32 {
        self.counter
    }
}

// 使用（必须 await）
let count = actor.send(GetCount).await.unwrap();
println!("Count: {}", count);
```

### 返回 Result

```rust
#[derive(Debug)]
pub struct Calculate {
    pub a: i32,
    pub b: i32,
}

impl ActixMessage for Calculate {
    type Result = Result<i32, String>;  // 返回 Result
}

impl Handler<Calculate> for CalcActor {
    type Result = Result<i32, String>;
    
    fn handle(&mut self, msg: Calculate, _ctx: &mut Context<Self>) -> Result<i32, String> {
        if msg.b == 0 {
            Err("Division by zero".to_string())
        } else {
            Ok(msg.a / msg.b)
        }
    }
}

// 使用
match actor.send(Calculate { a: 10, b: 2 }).await {
    Ok(Ok(result)) => println!("Result: {}", result),
    Ok(Err(e)) => println!("Error: {}", e),
    Err(_) => println!("Actor error"),
}
```

## WebSocket 数据流中的 Actor

### 订阅者 Actor 示例

```rust
use yue::binance::bn_models::spot_websocket_stream::BinanceSpotWebSocketStreamResponse;
use yue::websocket::event_bus::Subscribe;

pub struct MyWebSocketSubscriber {
    trade_count: u64,
}

impl Actor for MyWebSocketSubscriber {
    type Context = Context<Self>;
    
    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("Subscriber started");
    }
    
    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("Subscriber stopped. Total trades: {}", self.trade_count);
    }
}

impl Handler<BinanceSpotWebSocketStreamResponse> for MyWebSocketSubscriber {
    type Result = ();
    
    fn handle(&mut self, msg: BinanceSpotWebSocketStreamResponse, _ctx: &mut Context<Self>) {
        match msg {
            BinanceSpotWebSocketStreamResponse::Trade(trade) => {
                self.trade_count += 1;
                if self.trade_count % 100 == 0 {
                    info!("Received {} trades", self.trade_count);
                }
            }
            _ => {}
        }
    }
}

// 使用
let subscriber = MyWebSocketSubscriber { trade_count: 0 }.start();
bus.do_send(Subscribe {
    subscriber: subscriber.recipient(),
});
```

## 定时任务 Actor

### CronActor（基于 cron 表达式）

```rust
use li::actix_jobs::{CronActor, AsyncRepeatTask};
use async_trait::async_trait;

#[derive(Clone)]
pub struct MyTask;

#[async_trait]
impl AsyncRepeatTask for MyTask {
    async fn execute(&self) -> Result<(), LiError> {
        info!("Executing task...");
        // 执行业务逻辑
        Ok(())
    }
    
    fn task_name(&self) -> &str {
        "MyTask"
    }
}

// 使用
let actor = CronActor::new(
    "0 0 * * * *",  // 每小时执行
    MyTask
).start();
```

### run_interval（定时执行）

```rust
use std::time::Duration;

impl Actor for MyActor {
    type Context = Context<Self>;
    
    fn started(&mut self, ctx: &mut Self::Context) {
        // 每5秒执行一次
        ctx.run_interval(Duration::from_secs(5), |act, _ctx| {
            act.do_periodic_task();
        });
    }
}

impl MyActor {
    fn do_periodic_task(&mut self) {
        info!("Periodic task executed");
    }
}
```

## 存储 Actor 示例

### 缓冲并批量写入

```rust
pub struct StorageActor {
    buffer: Vec<Data>,
    batch_size: usize,
}

impl Actor for StorageActor {
    type Context = Context<Self>;
    
    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.set_mailbox_capacity(5000);  // 增大邮箱容量
        
        // 定时刷新
        ctx.run_interval(Duration::from_secs(5), |act, _ctx| {
            act.flush();
        });
    }
}

impl Handler<Data> for StorageActor {
    type Result = ();
    
    fn handle(&mut self, data: Data, _ctx: &mut Context<Self>) {
        self.buffer.push(data);
        
        if self.buffer.len() >= self.batch_size {
            self.flush();
        }
    }
}

impl StorageActor {
    fn flush(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        
        info!("Flushing {} records", self.buffer.len());
        // 批量写入数据库
        self.buffer.clear();
    }
}
```

## 上下文操作

### 基本操作

```rust
use actix::Context;

impl Handler<MyMessage> for MyActor {
    type Result = ();
    
    fn handle(&mut self, msg: MyMessage, ctx: &mut Context<Self>) {
        // 1. 停止 Actor
        ctx.stop();
        
        // 2. 延迟执行
        ctx.run_later(Duration::from_secs(1), |act, _ctx| {
            info!("Delayed action");
        });
        
        // 3. 定时执行
        ctx.run_interval(Duration::from_secs(5), |act, _ctx| {
            info!("Periodic action");
        });
        
        // 4. 获取当前地址
        let addr = ctx.address();
        
        // 5. 设置邮箱容量
        ctx.set_mailbox_capacity(10000);
    }
}
```

## 多 Actor 通信

### 发送消息给其他 Actor

```rust
#[derive(Debug)]
pub struct NotifyOther {
    pub target: Recipient<MyMessage>,
    pub data: String,
}

impl ActixMessage for NotifyOther {
    type Result = ();
}

impl Handler<NotifyOther> for MyActor {
    type Result = ();
    
    fn handle(&mut self, msg: NotifyOther, _ctx: &mut Context<Self>) {
        // 发送给其他 Actor
        msg.target.do_send(MyMessage {
            data: msg.data,
        });
    }
}
```

### 获取 Actor 地址

```rust
// 启动时
let actor_addr = MyActor.start();

// 获取 Recipient（可序列化/传递）
let recipient = actor_addr.recipient::<MyMessage>();

// 发送消息
recipient.do_send(MyMessage { data: "test".to_string() });
```

## 邮箱和背压

### 邮箱满时的处理

```rust
impl Handler<Message> for MyActor {
    type Result = ();
    
    fn handle(&mut self, msg: Message, _ctx: &mut Context<Self>) {
        // 使用 try_send 检测邮箱是否满
        if self.some_recipient.try_send(msg.clone()).is_err() {
            // 邮箱满了
            warn!("Mailbox full, using do_send to ensure delivery");
            self.some_recipient.do_send(msg);
        }
    }
}
```

### 设置邮箱容量

```rust
impl Actor for StorageActor {
    type Context = Context<Self>;
    
    fn started(&mut self, ctx: &mut Self::Context) {
        // 增加邮箱容量以处理高频消息
        ctx.set_mailbox_capacity(10000);
        
        info!("StorageActor started with mailbox capacity: 10000");
    }
}
```


### 注册监听消息


原则上，在初始化时，就完成订阅操作。比如把Recipient传入等。

除非非常必要。否则不要用订阅以下模式，只有在非常必要的前提下，采用以下模式。
凡事需要pub/sub等操作，则按照以下的方式执行。具体参考li/src/actix_jobs.rs
- ExampleEvent 根据需要取名和加减字段

1. 创建pub

```rust

#[derive(Debug, Clone)]
pub struct ExampleEvent {
    pub task_name: String, 
    pub result: Result<(), String>,
}

impl ActixMessage for ExampleEvent {
    type Result = ();
}

struct ExampleActor{
    subscribers: Vec<Recipient<ExampleEvent>>, // 订阅者列表
}

impl  Handler<SubscribeEvent<ExampleEvent>> for ExampleActor<T> {
    type Result = ();

    fn handle(&mut self, msg: SubscribeEvent<ExampleEvent>, _ctx: &mut Context<Self>) -> Self::Result {
        self.subscribers.push(msg.0);
        info!(
            "Subscriber registered for task '{}'. Total subscribers: {}",
            self.task.task_name(),
            self.subscribers.len()
        );
    }
}
```

2. 创建sub
```rust

let events = Arc::new(Mutex::new(Vec::new()));
let subscriber = TestSubscriber { events: events.clone() }.start();

subscribe_event!(addr, subscriber.recipient(), TaskCompletionEvent);
```
## 错误处理

### Actor 崩溃恢复

```rust
use actix::Supervised;

pub struct RobustActor;

impl Actor for RobustActor {
    type Context = Context<Self>;
}

// 实现 Supervised 以支持自动重启
impl Supervised for RobustActor {}

impl Handler<Message> for RobustActor {
    type Result = ();
    
    fn handle(&mut self, _msg: Message, _ctx: &mut Context<Self>) {
        // 即使这里 panic，Actor 也会自动重启
    }
}
```

### 错误传播

```rust
#[derive(Debug)]
pub struct Operation {
    pub value: i32,
}

impl ActixMessage for Operation {
    type Result = Result<i32, String>;
}

impl Handler<Operation> for MyActor {
    type Result = Result<i32, String>;
    
    fn handle(&mut self, msg: Operation, _ctx: &mut Context<Self>) -> Result<i32, String> {
        if msg.value < 0 {
            return Err("Negative value not allowed".to_string());
        }
        Ok(msg.value * 2)
    }
}
```

## 完整示例：实时计数器

```rust
use actix::{Actor, Context, Handler, Message as ActixMessage};
use std::time::Duration;

#[derive(Debug)]
pub struct Increment;

impl ActixMessage for Increment {
    type Result = ();
}

#[derive(Debug)]
pub struct GetCount;

impl ActixMessage for GetCount {
    type Result = u32;
}

pub struct CounterActor {
    count: u32,
}

impl Actor for CounterActor {
    type Context = Context<Self>;
    
    fn started(&mut self, ctx: &mut Self::Context) {
        println!("Counter started");
        
        // 每秒自动递增
        ctx.run_interval(Duration::from_secs(1), |act, _ctx| {
            act.count += 1;
            if act.count % 10 == 0 {
                println!("Counter: {}", act.count);
            }
        });
    }
}

impl Handler<Increment> for CounterActor {
    type Result = ();
    
    fn handle(&mut self, _msg: Increment, _ctx: &mut Context<Self>) {
        self.count += 1;
    }
}

impl Handler<GetCount> for CounterActor {
    type Result = u32;
    
    fn handle(&mut self, _msg: GetCount, _ctx: &mut Context<Self>) -> u32 {
        self.count
    }
}

#[actix::main]
async fn main() {
    let counter = CounterActor { count: 0 }.start();
    
    // 发送消息
    counter.do_send(Increment);
    
    // 查询计数
    let count = counter.send(GetCount).await.unwrap();
    println!("Current count: {}", count);
    
    tokio::time::sleep(Duration::from_secs(5)).await;
}
```
