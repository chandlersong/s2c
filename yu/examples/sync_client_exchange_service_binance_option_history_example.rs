use li::tools::logs::setup_logger;
use li::tools::time::unix_time_now_u64_utc;
use log::{LevelFilter, info};
use std::collections::HashMap;
use tonic::Request;
use yu::errors::YuError;
use yu::sync::client::database::initial_grpc_client_tables;
use yu::sync::client::grpc_manager::get_grpc_manager_from_config;
use yu::sync::client::service::binance::BinanceOptionClientInstrumentService;
use yu::sync::models::grpc_sync::{Empty, InstrumentType, sync_interface_client::SyncInterfaceClient};

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let mut special_log = HashMap::new();
    special_log.insert(
        "sync_client_exchange_service_binance_option_history_example".to_string(),
        LevelFilter::Debug,
    );
    setup_logger(Some(LevelFilter::Warn), special_log)?;

    initial_grpc_client_tables(None).await?;
    let client_service = BinanceOptionClientInstrumentService::for_production()?;
    let history_services = client_service.supported_history_services()?;
    let grpc_manager = get_grpc_manager_from_config();
    let channel = grpc_manager.get_channel().await;
    let mut server = SyncInterfaceClient::new(channel);
    let instruments = server.list_instrument(Request::new(Empty {})).await?.into_inner();
    let end_ms = unix_time_now_u64_utc();

    let mut option_instruments = 0;
    for instrument in instruments.instruments {
        if instrument.instrument_type != InstrumentType::BinanceOption as i32 {
            continue;
        }
        client_service.reconcile_instrument(instrument.clone()).await?;
        client_service.initial_data(instrument, end_ms).await?;
        option_instruments += 1;
    }

    for (history_type, history_service) in history_services {
        history_service.reconcile_history_data(end_ms).await?;
        info!("Reconciled Binance Option history: {history_type:?}");
    }
    info!("Initialized {option_instruments} Binance Option instruments");
    Ok(())
}
