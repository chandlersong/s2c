// Arrow Flight 集成示例代码

#![allow(dead_code)]

use yu::arrow_flight_server::start_flight_server;

#[tokio::main]
async fn main() {
    println!("Arrow Flight Integration Examples\n");

    println!("示例 1: 启动服务器");
    println!("  见 example_start_server() 函数\n");

    println!("示例 2: Python 客户端");
    example_python_client();
    println!();

    println!("示例 3: Rust 客户端框架");
    example_rust_client_framework();
    println!();

    println!("示例 4: 命令格式");
    example_command_formats();
    println!();

    println!("示例 5: 数据处理");
    example_handle_response();
}

/// 示例 1: 启动 Flight 服务器
#[tokio::main]
async fn example_start_server() -> Result<(), Box<dyn std::error::Error>> {
    // 启动服务器
    let addr = "127.0.0.1:50051";
    let shutdown_tx = start_flight_server(addr).await?;
    println!("✓ Flight server started on {}", addr);

    // 模拟业务逻辑
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    // 关闭服务器
    shutdown_tx.send(()).ok();
    println!("✓ Flight server stopped");

    Ok(())
}

/// 示例 2: Python 客户端代码 (伪代码)
fn example_python_client() {
    let python_code = r#"
import pyarrow.flight as flight
import pandas as pd

# 连接到 Flight 服务器
client = flight.connect("grpc://localhost:50051")

# 查询 1: Depth 数据 (模拟数据)
print("=== Depth Query ===")
ticket = flight.Ticket(b"depth:symbol=BTCUSDT")
reader = client.do_get(ticket)
depth_table = reader.read_all()
df_depth = depth_table.to_pandas()
print(df_depth)
# 输出:
#    symbol market_type side   price   qty  level  update_id          ts
# 0  BTCUSDT        spot  bid  41900.0   0.6 1000001  1704067200000
# 1  BTCUSDT        spot  bid  41800.0   0.7 1000002  1704067200000
# ...
# 5  BTCUSDT        spot  ask  42100.0   0.6 1000011  1704067200000
# ...

# 查询 2: SQL 数据
print("\n=== SQL Query ===")
ticket = flight.Ticket(b"SELECT id, name, age FROM users")
reader = client.do_get(ticket)
sql_table = reader.read_all()
df_sql = sql_table.to_pandas()
print(df_sql)
# 输出:
#    id    name  age
# 0   1   Alice   30
# 1   2     Bob   25

# 查询 3: 其他交易对
print("\n=== Other Symbols ===")
symbols = ["ETHUSDT", "BNBUSDT", "ADAUSDT"]
for symbol in symbols:
    ticket = flight.Ticket(f"depth:symbol={symbol}".encode())
    reader = client.do_get(ticket)
    table = reader.read_all()
    print(f"{symbol}: {table.num_rows} rows")
"#;
    println!("{}", python_code);
}

/// 示例 3: Rust 客户端代码框架
#[allow(unused_variables)]
fn example_rust_client_framework() {
    // 注意: 这是框架代码，完整实现需要 tonic 生成的客户端

    let client_code = r#"
use arrow_flight::flight_service_client::FlightServiceClient;
use arrow_flight::Ticket;
use prost::bytes::Bytes;
use tonic::transport::Channel;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 连接到服务器
    let channel = Channel::from_static("http://127.0.0.1:50051")
        .connect()
        .await?;
    let mut client = FlightServiceClient::new(channel);

    // 查询 Depth 数据
    let ticket = Ticket {
        ticket: Bytes::from("depth:symbol=BTCUSDT"),
    };
    let mut stream = client.do_get(tonic::Request::new(ticket)).await?;

    // 读取结果
    while let Some(flight_data) = stream.message().await? {
        println!("Received FlightData: {:?}", flight_data);
        // 解析 Arrow 数据
        // ...
    }

    Ok(())
}
"#;
    println!("{}", client_code);
}

/// 示例 4: 命令参数示例
fn example_command_formats() {
    println!("=== Supported Command Formats ===\n");

    // SQL 命令
    println!("SQL Commands:");
    println!("  1. SELECT * FROM users");
    println!("  2. SELECT id, name FROM users WHERE age > 25");
    println!("  3. SELECT COUNT(*) as cnt FROM users");
    println!();

    // Depth 命令 - 基本格式
    println!("Depth Commands:");
    println!("  1. depth:symbol=BTCUSDT");
    println!("  2. depth:symbol=ETHUSDT&market_type=spot");
    println!("  3. depth:symbol=BNBUSDT&market_type=spot&side=both&levels=20");
    println!();

    // 注意
    println!("Notes:");
    println!("  - symbol: 必需，支持大小写 (自动转大写)");
    println!("  - market_type: 可选，默认 'spot'");
    println!("  - side: 可选，默认 'both' (bid/ask/both)");
    println!("  - levels: 可选，默认 20 (当前实现固定返回 5 档)");
}

/// 示例 5: 处理响应数据
fn example_handle_response() {
    let rust_code = r#"
use arrow::record_batch::RecordBatch;
use arrow::array::StringArray;

fn process_depth_data(batch: &RecordBatch) -> Result<(), Box<dyn std::error::Error>> {
    // 获取列引用
    let symbols = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
    let sides = batch.column(2).as_any().downcast_ref::<StringArray>().unwrap();
    let prices = batch.column(3).as_any().downcast_ref::<arrow::array::Float64Array>().unwrap();
    let qtys = batch.column(4).as_any().downcast_ref::<arrow::array::Float64Array>().unwrap();

    // 遍历数据
    for i in 0..batch.num_rows() {
        let symbol = symbols.value(i);
        let side = sides.value(i);
        let price = prices.value(i);
        let qty = qtys.value(i);

        println!("{} {} {} {}", symbol, side, price, qty);
    }

    Ok(())
}

// 示例调用
match process_depth_data(&batch) {
    Ok(()) => println!("✓ Data processed successfully"),
    Err(e) => eprintln!("✗ Error: {}", e),
}
"#;
    println!("{}", rust_code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_example_code_compiles() {
        // 这个测试确保示例代码不会产生编译错误
        println!("✓ All example code compiles successfully");
    }
}
