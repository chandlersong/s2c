use li::tools::logs::setup_logger;
use li::tools::time::unix_time_now_u64_utc;
use log::{LevelFilter, info};
use std::collections::HashMap;
use tonic::Request;
use yu::config::get_config;
use yu::errors::YuError;
use yu::sync::client::database::initial_grpc_client_tables;
use yu::sync::client::grpc_manager::get_grpc_manager_from_config;
use yu::sync::client::service::binance::BinanceOptionClientInstrumentService;
use yu::sync::models::grpc_sync::Empty;
use yu::sync::models::grpc_sync::instrument::Payload;
use yu::sync::models::grpc_sync::sync_interface_client::SyncInterfaceClient;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let mut special_log = HashMap::new();
    special_log.insert("li".to_string(), LevelFilter::Trace);
    special_log.insert("yu".to_string(), LevelFilter::Trace);
    special_log.insert("client_exchange_service_binance_option_example".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Warn), special_log)?;
    let _app_config = get_config();

    initial_grpc_client_tables(None).await?;
    let instrument_service = BinanceOptionClientInstrumentService::for_production()?;
    let connection_manager = get_grpc_manager_from_config();
    let mut server = SyncInterfaceClient::new(connection_manager.connect().await);
    let response = server.list_instrument(Request::new(Empty {})).await?;
    let instrument_id = match std::env::var("BINANCE_OPTION_SERVER_ID") {
        Ok(value) => Some(
            value
                .parse::<u64>()
                .map_err(|error| YuError::new(&format!("invalid BINANCE_OPTION_SERVER_ID: {error}")))?,
        ),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(YuError::new(&format!("failed to read BINANCE_OPTION_SERVER_ID: {error}"))),
    };
    let instrument = response.into_inner().instruments.into_iter().find(|instrument| {
        matches!(
            instrument.payload.as_ref(),
            Some(Payload::BinanceOptionInstrument(option)) if instrument_id.is_none_or(|server_id| option.server_id == server_id)
        )
    });
    let Some(instrument) = instrument else {
        return Err(YuError::new("no Binance Option instrument found in server instrument list"));
    };

    info!("对账 Binance Option instrument: {:?}", instrument.payload);
    instrument_service.reconcile_instrument(instrument.clone()).await?;
    instrument_service.initial_data(instrument, unix_time_now_u64_utc()).await?;
    info!("Binance Option instrument client example completed");
    Ok(())
}
