use arrow_flight::flight_service_server::FlightServiceServer;
use li::tools::logs::setup_logger_all;
use log::LevelFilter;
use tonic::transport::Server;
use yu::arrow_flight_server::DuckDBFlightServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logger_all(Some(LevelFilter::Info)).unwrap();
    let addr = "0.0.0.0:8815".parse()?;
    let service = DuckDBFlightServer::new();

    let svc = FlightServiceServer::new(service);
    Server::builder().add_service(svc).serve(addr).await?;

    Ok(())
}
