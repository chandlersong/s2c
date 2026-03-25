use crate::binance::bn_dashboard::{get_market_depth_dashboard, QueryDepth};
use crate::duck_db::DBProvider;
use crate::errors::YuError;
use arrow::array::{ArrayRef, Float64Array, Int64Array, StringArray, UInt32Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use arrow_flight::flight_service_server::{FlightService, FlightServiceServer};
use arrow_flight::utils as flight_utils;
use arrow_flight::{
    Action, ActionType, Criteria, Empty, FlightData, FlightDescriptor, FlightInfo, HandshakeRequest, HandshakeResponse, PollInfo, PutResult,
    SchemaResult, Ticket,
};
use duckdb::params;
use log::{debug, error, info};
use prost::bytes::Bytes;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Server;
use tonic::{Request, Response, Status, Streaming};
use yue::binance::order_book::OrderBook;

///
/// 想要作为数据中心，以后会支持很多方法
/// 现在只是支持SQL查询
#[derive(Clone)]
pub struct DuckDBFlightServer {
    db_provider: DBProvider,
}

// 命令类型枚举
#[derive(Debug)]
enum CommandType {
    Sql(String),
    Depth(String), // symbol
}

// 解析 ticket 字符串，区分 SQL 和 Depth 命令
fn parse_command(ticket_str: &str) -> Result<CommandType, String> {
    let trimmed = ticket_str.trim();

    if trimmed.starts_with("depth:") {
        // 提取 symbol 参数：depth:symbol=BTCUSDT
        let params_str = &trimmed[6..]; // 去掉 "depth:" 前缀
        if let Some(start) = params_str.find("symbol=") {
            let symbol_part = &params_str[start + 7..]; // 去掉 "symbol="
            let symbol = symbol_part.split('&').next().unwrap_or(symbol_part).to_uppercase();
            if symbol.is_empty() {
                return Err("Empty symbol in depth command".to_string());
            }
            Ok(CommandType::Depth(symbol))
        } else {
            Err("Missing symbol parameter in depth command".to_string())
        }
    } else {
        // 默认作为 SQL 命令
        Ok(CommandType::Sql(trimmed.to_string()))
    }
}

fn convert_order_book_to_record_batch(order_book: &OrderBook, levels: Option<u16>) -> Result<RecordBatch, String> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("symbol", DataType::Utf8, false),
        Field::new("market_type", DataType::Utf8, false),
        Field::new("side", DataType::Utf8, false),
        Field::new("price", DataType::Float64, false),
        Field::new("qty", DataType::Float64, false),
        Field::new("level", DataType::UInt32, false),
        Field::new("update_id", DataType::Int64, false),
        Field::new("ts", DataType::Int64, false),
    ]));

    let order_book_to_use = if let Some(lvls) = levels {
        order_book
            .get_sub_order_book(lvls)
            .map_err(|e| format!("Failed to get sub order book: {}", e))?
    } else {
        order_book.clone()
    };

    let mut symbols = vec![];
    let mut market_types = vec![];
    let mut sides = vec![];
    let mut prices = vec![];
    let mut qtys = vec![];
    let mut level_nums = vec![];
    let mut update_ids = vec![];
    let mut tss = vec![];

    let mut level = 1u32;
    for (price, qty) in order_book_to_use.bids().iter().rev() {
        symbols.push(order_book_to_use.symbol.clone());
        market_types.push("spot".to_string());
        sides.push("bid".to_string());
        prices.push(price.to_f64().ok_or_else(|| "Failed to convert bid price to f64".to_string())?);
        qtys.push(qty.to_f64().ok_or_else(|| "Failed to convert bid qty to f64".to_string())?);
        level_nums.push(level);
        update_ids.push(order_book_to_use.local_update_id as i64);
        tss.push(order_book_to_use.last_update_time as i64);
        level += 1;
    }

    level = 1u32;
    for (price, qty) in order_book_to_use.asks().iter() {
        symbols.push(order_book_to_use.symbol.clone());
        market_types.push("spot".to_string());
        sides.push("ask".to_string());
        prices.push(price.to_f64().ok_or_else(|| "Failed to convert ask price to f64".to_string())?);
        qtys.push(qty.to_f64().ok_or_else(|| "Failed to convert ask qty to f64".to_string())?);
        level_nums.push(level);
        update_ids.push(order_book_to_use.local_update_id as i64);
        tss.push(order_book_to_use.last_update_time as i64);
        level += 1;
    }

    let columns: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from(symbols)),
        Arc::new(StringArray::from(market_types)),
        Arc::new(StringArray::from(sides)),
        Arc::new(Float64Array::from(prices)),
        Arc::new(Float64Array::from(qtys)),
        Arc::new(UInt32Array::from(level_nums)),
        Arc::new(Int64Array::from(update_ids)),
        Arc::new(Int64Array::from(tss)),
    ];

    RecordBatch::try_new(schema, columns).map_err(|e| format!("Failed to create RecordBatch: {}", e))
}

impl DuckDBFlightServer {
    pub fn new() -> Self {
        DuckDBFlightServer {
            db_provider: DBProvider::default(),
        }
    }
}

#[tonic::async_trait]
impl FlightService for DuckDBFlightServer {
    type HandshakeStream = ReceiverStream<Result<HandshakeResponse, Status>>;
    async fn handshake(&self, _request: Request<Streaming<HandshakeRequest>>) -> Result<Response<Self::HandshakeStream>, Status> {
        // Flight 握手认证接口，未实现
        Err(Status::unimplemented("Implement handshake"))
    }
    type ListFlightsStream = ReceiverStream<Result<FlightInfo, Status>>;
    async fn list_flights(&self, _request: Request<Criteria>) -> Result<Response<Self::ListFlightsStream>, Status> {
        // Flight 查询所有可用数据集接口，未实现
        Err(Status::unimplemented("Implement list_flights"))
    }
    async fn get_flight_info(&self, request: Request<FlightDescriptor>) -> Result<Response<FlightInfo>, Status> {
        // 解析请求，获取 FlightDescriptor
        let descriptor = request.into_inner();
        debug!("Got flight info: {:?}", descriptor);

        // 从 descriptor 中提取命令字符串（通常在 cmd 字段或 path 中）
        let command_str = if !descriptor.path.is_empty() {
            descriptor.path.join("/")
        } else {
            String::new()
        };

        info!(
            "get_flight_info command: {}",
            if command_str.is_empty() { "default" } else { &command_str }
        );

        // 构造一个 FlightEndpoint，包含一个 Ticket 和地址
        let ticket_bytes = if command_str.is_empty() {
            Bytes::from("simple_ticket")
        } else {
            Bytes::from(command_str.clone())
        };

        let endpoint = arrow_flight::FlightEndpoint {
            ticket: Some(Ticket { ticket: ticket_bytes }),
            // 构造 Location 直接通过字段（generated code 中通常为 `uri: String`）
            location: vec![arrow_flight::Location {
                uri: "grpc+tcp://localhost:50051".to_string(),
            }],
            // expiration_time 在 proto 中可能为 optional int64
            expiration_time: None,
            // app_metadata 在生成的类型是 Bytes（非 Option），使用空 Bytes
            app_metadata: Bytes::new(),
        };

        // 根据命令类型判断并记录日志
        match parse_command(&command_str) {
            Ok(CommandType::Depth(_)) => {
                info!("Depth query detected, schema will be returned in do_get");
            }
            Ok(CommandType::Sql(_)) => {
                info!("SQL query detected, schema will be returned in do_get");
            }
            Err(e) => {
                info!("Command parse error: {}", e);
            }
        }

        // 构造 FlightInfo，schema 在 do_get 中动态生成
        let flight_info = FlightInfo {
            schema: Bytes::new(), // schema 在 do_get 中返回
            flight_descriptor: Some(descriptor),
            endpoint: vec![endpoint],
            total_records: -1,
            total_bytes: -1,
            ordered: false,
            // FlightInfo.app_metadata 是 Bytes（非 Option）
            app_metadata: Bytes::new(),
        };
        // 返回 Response<FlightInfo>
        Ok(Response::new(flight_info))
    }
    async fn poll_flight_info(&self, _request: Request<FlightDescriptor>) -> Result<Response<PollInfo>, Status> {
        // Flight 查询数据集状态接口，未实现
        Err(Status::unimplemented("Implement poll_flight_info"))
    }
    async fn get_schema(&self, _request: Request<FlightDescriptor>) -> Result<Response<SchemaResult>, Status> {
        // Flight 查询数据集 schema 接口，未实现
        Err(Status::unimplemented("Implement get_schema"))
    }
    type DoGetStream = ReceiverStream<Result<FlightData, Status>>;
    async fn do_get(&self, request: Request<Ticket>) -> Result<Response<Self::DoGetStream>, Status> {
        let ticket = request.into_inner();
        let ticket_str = String::from_utf8(ticket.ticket.to_vec()).map_err(|_| Status::invalid_argument("Invalid ticket"))?;
        info!("Got do_get:{}", ticket_str);

        if ticket_str.trim().is_empty() {
            return Err(Status::invalid_argument("Empty ticket"));
        }

        let (tx, rx) = mpsc::channel::<Result<FlightData, Status>>(32);
        let tx_clone = tx.clone();

        // 解析命令
        match parse_command(&ticket_str) {
            Ok(CommandType::Depth(symbol)) => {
                info!("Processing depth command for symbol: {}", symbol);
                tokio::task::spawn(async move {
                    let result: Result<(), String> = async {
                        let dashboard = get_market_depth_dashboard().map_err(|e| format!("Failed to get MarketDepthDashBoard: {}", e))?;

                        let order_book_arc = dashboard
                            .send(QueryDepth { symbol: symbol.clone() })
                            .await
                            .map_err(|e| format!("Failed to send query to dashboard: {}", e))?;

                        let order_book = order_book_arc.ok_or_else(|| format!("OrderBook not found for symbol: {}", symbol))?;

                        let batch = convert_order_book_to_record_batch(&order_book, Some(20))?;
                        let schema = batch.schema();
                        let flight_data_vec = flight_utils::batches_to_flight_data(schema.as_ref(), vec![batch])
                            .map_err(|e| format!("Failed to convert batches to FlightData: {}", e))?;

                        for d in flight_data_vec {
                            if let Err(send_err) = tx_clone.send(Ok(d)).await {
                                return Err(format!("Failed to send FlightData: {}", send_err));
                            }
                        }
                        Ok(())
                    }
                    .await;

                    if let Err(err_msg) = result {
                        let status = Status::internal(err_msg.clone());
                        let _ = tx_clone.send(Err(status)).await;
                        error!("Depth command failed: {}", err_msg);
                    }
                    drop(tx_clone);
                });
            }
            Ok(CommandType::Sql(sql)) => {
                // 处理 SQL 命令
                let db_provider = self.db_provider.clone();
                info!("Processing SQL command");
                tokio::task::spawn_blocking(move || {
                    let result: Result<(), String> = (|| {
                        let conn = db_provider.acquire().map_err(|e| format!("DB acquire error: {}", e))?;

                        let mut stmt = conn.prepare(&sql).map_err(|e| format!("Prepare error: {}", e))?;
                        let mut arrow_result = stmt.query_arrow(params![]).map_err(|e| format!("Query arrow error: {}", e))?;

                        let mut batches: Vec<RecordBatch> = Vec::new();
                        while let Some(batch) = arrow_result.next() {
                            batches.push(batch);
                        }

                        let schema = arrow_result.get_schema();

                        let flight_data_vec = flight_utils::batches_to_flight_data(schema.as_ref(), batches)
                            .map_err(|e| format!("Failed to convert batches to FlightData: {}", e))?;

                        for d in flight_data_vec {
                            if let Err(send_err) = tx_clone.blocking_send(Ok(d)) {
                                return Err(format!("Failed to send FlightData to receiver: {}", send_err));
                            }
                        }

                        Ok(())
                    })();

                    if let Err(err_msg) = result {
                        let status = Status::internal(err_msg.clone());
                        let _ = tx_clone.blocking_send(Err(status));
                        error!("SQL command failed: {}", err_msg);
                    }

                    drop(tx_clone);
                });
            }
            Err(err_msg) => {
                return Err(Status::invalid_argument(format!("Command parsing failed: {}", err_msg)));
            }
        }

        Ok(Response::new(ReceiverStream::new(rx)))
    }
    type DoPutStream = ReceiverStream<Result<PutResult, Status>>;
    async fn do_put(&self, _request: Request<Streaming<FlightData>>) -> Result<Response<Self::DoPutStream>, Status> {
        // Flight 上传数据接口，未实现
        Err(Status::unimplemented("Implement do_put"))
    }
    type DoExchangeStream = ReceiverStream<Result<FlightData, Status>>;
    async fn do_exchange(&self, _request: Request<Streaming<FlightData>>) -> Result<Response<Self::DoExchangeStream>, Status> {
        // Flight 双向数据交换接口，未实现
        Err(Status::unimplemented("Implement do_exchange"))
    }
    type DoActionStream = ReceiverStream<Result<arrow_flight::Result, Status>>;
    async fn do_action(&self, _request: Request<Action>) -> Result<Response<Self::DoActionStream>, Status> {
        // Flight 执行自定义操作接口，未实现
        Err(Status::unimplemented("Implement do_action"))
    }
    type ListActionsStream = ReceiverStream<Result<ActionType, Status>>;
    async fn list_actions(&self, _request: Request<Empty>) -> Result<Response<Self::ListActionsStream>, Status> {
        // Flight 查询支持的自定义操作接口，未实现
        Err(Status::unimplemented("Implement list_actions"))
    }
}

pub async fn start_flight_server(addr: &str) -> Result<oneshot::Sender<()>, YuError> {
    let addr_str = addr.to_string();
    let parsed_addr = addr_str.parse().map_err(|e| YuError::new(&format!("Bad address {}: {}", addr, e)))?;

    // Create a shutdown channel for the server
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // Create initialization result channel - only used to report startup errors
    let (init_tx, mut init_rx) = tokio::sync::mpsc::channel::<Result<(), String>>(1);

    let init_tx_clone = init_tx.clone();

    // Spawn the tonic server task
    tokio::spawn(async move {
        let service = DuckDBFlightServer::new();
        let svc = FlightServiceServer::new(service);

        let shutdown_future = async {
            let _ = shutdown_rx.await;
            info!("Shutdown signal received for flight server");
        };

        // Try to start the server
        match Server::builder().add_service(svc).serve_with_shutdown(parsed_addr, shutdown_future).await {
            Ok(_) => {
                info!("✅ Arrow Flight Server shutdown gracefully");
            }
            Err(e) => {
                let error_msg = format!("Arrow Flight Server error: {}", e);
                error!("❌ {}", error_msg);
                // Notify about the error
                let _ = init_tx_clone.send(Err(error_msg)).await;
            }
        }
    });

    // Spawn a health check task to verify server is running
    let addr_str_health = addr_str.clone();
    tokio::spawn(async move {
        // Give the server a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Try to connect to verify server is running
        let max_retries = 50; // 5 seconds with 100ms intervals
        for attempt in 0..max_retries {
            match tokio::net::TcpStream::connect(&addr_str_health).await {
                Ok(_) => {
                    info!("✅ Arrow Flight Server is running and accepting connections on {}", &addr_str_health);
                    // Server is confirmed to be running, close the verification connection implicitly
                    return;
                }
                Err(e) => {
                    if attempt == max_retries - 1 {
                        let error_msg = format!("Arrow Flight Server failed to start after {} attempts: {}", max_retries, e);
                        error!("❌ {}", error_msg);
                        let _ = init_tx.send(Err(error_msg)).await;
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        }
    });

    // Wait for either a startup error or timeout
    tokio::select! {
        result = init_rx.recv() => {
            if let Some(Err(e)) = result {
                return Err(YuError::new(&e));
            }
        }
        _ = tokio::time::sleep(tokio::time::Duration::from_secs(6)) => {
            // If we don't get an error message within 6 seconds, assume success
            info!("Arrow Flight Server initialization completed (6s timeout)");
        }
    }

    Ok(shutdown_tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duck_db::DBProvider;
    use crate::errors::YuError;
    use crate::test_utils::initial_memory_db;
    use arrow_flight::utils as flight_utils;
    use duckdb::params;

    #[test]
    fn test_parse_depth_command() {
        // 测试深度命令解析
        let cmd = "depth:symbol=BTCUSDT&market_type=spot&side=both&levels=20";
        match parse_command(cmd) {
            Ok(CommandType::Depth(symbol)) => {
                assert_eq!(symbol, "BTCUSDT");
                println!("✓ 深度命令解析成功: {}", symbol);
            }
            _ => panic!("深度命令解析失败"),
        }
    }

    #[test]
    fn test_parse_depth_command_lowercase_symbol() {
        // 测试小写 symbol 转大写
        let cmd = "depth:symbol=ethusdt";
        match parse_command(cmd) {
            Ok(CommandType::Depth(symbol)) => {
                assert_eq!(symbol, "ETHUSDT");
                println!("✓ 小写 symbol 正确转大写: {}", symbol);
            }
            _ => panic!("深度命令解析失败"),
        }
    }

    #[test]
    fn test_parse_sql_command() {
        // 测试 SQL 命令解析
        let cmd = "SELECT * FROM users";
        match parse_command(cmd) {
            Ok(CommandType::Sql(sql)) => {
                assert_eq!(sql, "SELECT * FROM users");
                println!("✓ SQL 命令解析成功: {}", sql);
            }
            _ => panic!("SQL 命令解析失败"),
        }
    }

    #[test]
    fn test_parse_depth_missing_symbol() {
        // 测试缺失 symbol 参数
        let cmd = "depth:market_type=spot";
        match parse_command(cmd) {
            Err(msg) => {
                assert!(msg.contains("Missing symbol"));
                println!("✓ 正确捕获缺失 symbol 的错误: {}", msg);
            }
            _ => panic!("应该返回缺失参数错误"),
        }
    }

    #[tokio::test]
    async fn test_initial_flight_server() -> Result<(), YuError> {
        // 初始化内存数据库连接并建表
        let pool = initial_memory_db();
        let db_provider = DBProvider::new(pool);
        let conn = db_provider.acquire()?;
        let sql = "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name VARCHAR, age INTEGER);";
        let insert_sql = "INSERT INTO users (id,name, age) VALUES (1,'Alice', 30), (2,'Bob', 25);";
        conn.execute(sql, [])?;
        conn.execute(insert_sql, [])?;
        let mut stmt = conn.prepare("SELECT table_name FROM information_schema.tables WHERE table_schema = 'main'")?;
        let table_names: Vec<String> = stmt.query_map([], |row| row.get(0))?.collect::<Result<_, _>>()?;

        for table_name in table_names {
            println!("Table: {}", table_name);

            // 获取表schema
            let mut stmt = conn.prepare("select * from users")?;
            let mut arrow_result = stmt.query_arrow(params![])?;
            // 使用 LIMIT 0 获取 schema（不实际返回数据）
            // 从 Arrow 结果中提取 schema
            let mut batches = Vec::new();
            while let Some(batch) = arrow_result.next() {
                batches.push(batch);
            }
            let schema = arrow_result.get_schema();
            println!("Schema: {:?}", schema);
            let flight_data_vec = flight_utils::batches_to_flight_data(schema.as_ref(), batches).unwrap();
            println!("{:?}", flight_data_vec);
            // // 输出Arrow Schema（Flight兼容格式）
        }

        Ok(())
    }
}
