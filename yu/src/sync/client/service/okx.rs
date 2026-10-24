use crate::errors::YuError;
use crate::postgresql_db::get_sync_client_pg_pool_sync;
use crate::sync::client::repository::okx::{ClientOkxRepository, ClientOkxRepositoryImpl};
use crate::sync::client::sync_client_service::{ClientHistoryService, ClientInstrumentService, ClientInstrumentServiceTrait};
use crate::sync::models::grpc_sync::{HistoryType, Instrument};
use async_trait::async_trait;
use li::tools::time::UnixTimeStamp;
use std::collections::HashMap;
use std::sync::Arc;

pub struct OkxHistoryClientService {
    repository: ClientOkxRepository,
}

// This service only receives history; instrument rows must already exist locally.
impl OkxHistoryClientService {
    pub fn for_production() -> Result<ClientInstrumentService, YuError> {
        let pool = get_sync_client_pg_pool_sync()?;
        Ok(Arc::new(Self {
            repository: ClientOkxRepositoryImpl::from_pool(pool),
        }))
    }
}

#[async_trait]
impl ClientInstrumentServiceTrait for OkxHistoryClientService {
    async fn initial_data(&self, instrument: Instrument, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        todo!()
    }

    async fn check_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError> {
        self.repository.find_instrument_id_by_server_id(server_id).await
    }

    fn supported_history_services(&self) -> Result<HashMap<HistoryType, ClientHistoryService>, YuError> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::client::repository::okx::MockClientOkxRepositoryTrait;

    #[tokio::test]
    async fn check_server_id_returns_local_instrument_id_when_found() {
        let mut repository = MockClientOkxRepositoryTrait::new();
        repository
            .expect_find_instrument_id_by_server_id()
            .withf(|server_id| *server_id == 42)
            .returning(|_| Ok(Some(123)));
        let service = OkxHistoryClientService {
            repository: Arc::new(repository),
        };

        assert_eq!(service.check_server_id(42).await.unwrap(), Some(123));
    }

    #[tokio::test]
    async fn check_server_id_returns_none_when_not_found() {
        let mut repository = MockClientOkxRepositoryTrait::new();
        repository
            .expect_find_instrument_id_by_server_id()
            .withf(|server_id| *server_id == 42)
            .returning(|_| Ok(None));
        let service = OkxHistoryClientService {
            repository: Arc::new(repository),
        };

        assert_eq!(service.check_server_id(42).await.unwrap(), None);
    }
}
