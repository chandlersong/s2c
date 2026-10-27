use li::tools::logs::setup_logger;
use li::tools::time::unix_time_now_u64_utc;
use log::{LevelFilter, info};
use std::collections::HashMap;
use tonic::Request;
use yu::config::get_config;
use yu::errors::YuError;
use yu::sync::client::database::initial_grpc_client_tables;
use yu::sync::client::grpc_manager::get_grpc_manager_from_config;
use yu::sync::client::service::binance::BinanceSpotClientInstrumentService;
use yu::sync::models::grpc_sync::Empty;
use yu::sync::models::grpc_sync::instrument::Payload;
use yu::sync::models::grpc_sync::sync_interface_client::SyncInterfaceClient;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let mut special_log = HashMap::new();
    special_log.insert("li".to_string(), LevelFilter::Trace);
    special_log.insert("yu".to_string(), LevelFilter::Trace);
    special_log.insert("sync_client_exchange_service_binance_swap_example".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Warn), special_log)?;
    let _app_config = get_config();

    initial_grpc_client_tables(None).await?;
    let binance_instrument_service = BinanceSpotClientInstrumentService::for_production()?;
    let connection_manager = get_grpc_manager_from_config();
    let mut server = SyncInterfaceClient::new(connection_manager.connect().await);
    let response = server.list_instrument(Request::new(Empty {})).await?;
    let instruments = response.into_inner().instruments;
    info!("server instrument count: {}", instruments.len());

    let Some(instrument) = instruments.into_iter().find(|instrument| {
        matches!(
            instrument.payload.as_ref(),
            Some(Payload::Binance(binance_instrument))
                if matches!(binance_instrument.symbol_type.to_ascii_uppercase().as_str(), "SWAP" | "PERPETUAL")
        )
    }) else {
        return Err(YuError::new("no Binance Swap instrument found in server instrument list"));
    };

    let end_ms = unix_time_now_u64_utc();
    binance_instrument_service.initial_data(instrument, end_ms).await?;
    for (history_type, history_service) in binance_instrument_service.supported_history_services()? {
        info!("reconcile history type: {}", history_type.readable_name());
        history_service.reconcile_living_instrument_with_server(end_ms).await?;
    }

    Ok(())
}
