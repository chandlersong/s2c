use crate::duck_db::DBProvider;
use crate::errors::YuError;
use arrow_flight::flight_service_server::{FlightService, FlightServiceServer};
use arrow_flight::utils as flight_utils;
use arrow_flight::{
    Action, ActionType, Criteria, Empty, FlightData, FlightDescriptor, FlightInfo, HandshakeRequest, HandshakeResponse, PollInfo, PutResult,
    SchemaResult, Ticket,
};
use duckdb::params;
use log::{debug, error, info};
use prost::bytes::Bytes;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Server;
use tonic::{Request, Response, Status, Streaming};

///
/// 想要作为数据中心，以后会支持很多方法
/// 现在只是支持SQL查询
#[derive(Clone)]
pub struct DuckDBFlightServer {
    db_provider: DBProvider,
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

        // 构造一个简单的 schema（如 int32 字段）
        // 构造一个 FlightEndpoint，包含一个 Ticket 和地址
        let endpoint = arrow_flight::FlightEndpoint {
            ticket: Some(Ticket {
                ticket: Bytes::from("simple_ticket"),
            }),
            // 构造 Location 直接通过字段（generated code 中通常为 `uri: String`）
            location: vec![arrow_flight::Location {
                uri: "grpc+tcp://localhost:50051".to_string(),
            }],
            // expiration_time 在 proto 中可能为 optional int64
            expiration_time: None,
            // app_metadata 在生成的类型是 Bytes（非 Option），使用空 Bytes
            app_metadata: Bytes::new(),
        };
        // 构造 FlightInfo，填充 schema、descriptor、endpoint 等字段（将 Vec<u8> 转为 Bytes）
        let flight_info = FlightInfo {
            schema: Bytes::from(Vec::new()),
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

        let db_provider = self.db_provider.clone();

        // 在阻塞线程池中执行同步的 DB/Arrow 操作，避免把非-Send 的连接移入 async future
        tokio::task::spawn_blocking(move || {
            // 将所有内部错误映射为字符串，最后统一发送到 channel
            let result: Result<(), String> = (|| {
                // 在这里再 acquire，连接只在阻塞线程里使用
                let conn = db_provider.acquire().map_err(|e| format!("DB acquire error: {}", e))?;

                // 使用 ticket_str 作为 SQL（注意：这里假设 ticket 是一条 SQL；如果你的协议不同，请在外面解析再传入）
                let mut stmt = conn.prepare(&ticket_str).map_err(|e| format!("Prepare error: {}", e))?;
                let mut arrow_result = stmt.query_arrow(params![]).map_err(|e| format!("Query arrow error: {}", e))?;

                // 将 Arrow 结果收集为 RecordBatch
                let mut batches = Vec::new();
                while let Some(batch) = arrow_result.next() {
                    batches.push(batch);
                }

                // 从结果里获取 schema
                let schema = arrow_result.get_schema();

                // 将 RecordBatch 转为 FlightData（可能返回错误）
                let flight_data_vec =
                    flight_utils::batches_to_flight_data(&schema, batches).map_err(|e| format!("Failed to convert batches to FlightData: {}", e))?;

                // 发送每一条 FlightData 到异步接收端；如果发送失败，说明接收端已关闭（或出现其他问题），直接停止
                for d in flight_data_vec {
                    if let Err(send_err) = tx_clone.blocking_send(Ok(d)) {
                        return Err(format!("Failed to send FlightData to receiver: {}", send_err));
                    }
                }

                Ok(())
            })();

            // 如果有错误，尝试把错误作为 Status 发送给接收端，然后记录日志
            if let Err(err_msg) = result {
                let status = Status::internal(err_msg.clone());
                // best-effort: 不要让发送错误导致 panic
                let _ = tx_clone.blocking_send(Err(status));
                log::error!("do_get failed: {}", err_msg);
            }

            // 确保关闭发送端，这会让 ReceiverStream 终止
            drop(tx_clone);
        });

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

pub async fn start_flight_server(addr: &str) -> Result<tokio::sync::oneshot::Sender<()>, YuError> {
    let addr = addr.parse().map_err(|e| YuError::new(&format!("Bad address {}: {}", addr, e)))?;

    // Create a shutdown channel; return the sender to the caller so they can trigger shutdown
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // Spawn the tonic server on the current runtime. This avoids creating a new runtime/thread.
    tokio::spawn(async move {
        let service = DuckDBFlightServer::new();
        let svc = FlightServiceServer::new(service);

        let shutdown_future = async {
            // Wait for shutdown signal from the caller
            let _ = shutdown_rx.await;
            info!("Shutdown signal received for flight server");
        };

        if let Err(e) = Server::builder().add_service(svc).serve_with_shutdown(addr, shutdown_future).await {
            error!("Flight server error: {}", e);
        }
    });

    Ok(shutdown_tx)
}

#[cfg(test)]
mod tests {
    use crate::duck_db::DBProvider;
    use crate::errors::YuError;
    use crate::utils::initial_memory_db;
    use arrow_flight::utils as flight_utils;
    use duckdb::params;

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
            let flight_data_vec = flight_utils::batches_to_flight_data(&schema, batches).unwrap();
            println!("{:?}", flight_data_vec);
            // // 输出Arrow Schema（Flight兼容格式）
        }

        Ok(())
    }
}
