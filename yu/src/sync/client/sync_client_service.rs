use crate::errors::YuError;
use crate::postgresql_db::get_sync_client_pg_pool_sync;
use crate::sync::client::repository::{ClientPolyMarketRepository, ClientPolyMarketRepositoryImpl};
use crate::sync::client::po::LocalPolyMarketAssetInfoPo;
use crate::sync::sync_server::grpc_sync::{PolyMarketAssetInfoList, ServerMessage};
use log::info;
use std::collections::HashMap;
use std::sync::mpsc;

pub struct SyncClientService {
    repository: ClientPolyMarketRepository,
}

impl Default for SyncClientService {
    fn default() -> Self {
        Self {
            repository: ClientPolyMarketRepositoryImpl::from_pool(get_sync_client_pg_pool_sync().expect("get_sync_client_pg_pool_sync failed")),
        }
    }
}

impl SyncClientService {
    ///
    /// # 方法作用
    /// 校准本地数据库 asset info
    /// 1. 如果有新的asset info。那么就加入本地数据库
    /// 2. 返回一个map，为本地数据库的时间戳，和服务器端不一样的。
    ///
    /// # 步骤
    /// 1. 找出local_assets和server_assets的不同。做以下操纵。
    ///    1. 如果存在于server_assets中，但是不存在于local_assets中，
    ///       - 把server_asset转换成LocalPolyMarketAssetInfoPo，存入数据库
    ///       - 加入返回值，key为assert_id, value为0
    ///    2. 如果存在于local_assets中，但是不存在于server_assets中，跳过，不做任何操作
    ///    3，都存在的话，那么比较local_assets的timestamp和服务器端的timestamp
    ///        - 如果local的timestamp小于server端的timestamp。则加入返回值，key为asset_id,value为local的timestamp
    ///
    pub async fn align_local_assets(&self, server_assets: PolyMarketAssetInfoList) -> Result<HashMap<String, u64>, YuError> {
        let local_assets = self.repository.list_assets_timestamp().await?;
        info!("local assets num: {}", local_assets.len());

        let mut res: HashMap<String, u64> = HashMap::new();

        // server_assets.assets: map<string, PolymarketAssertInfo>
        for (_key, info) in server_assets.assets.into_iter() {
            // prefer info.asset_id if set, otherwise use map key
            let asset_id = if !info.asset_id.is_empty() { info.asset_id.clone() } else { _key.clone() };

            if !local_assets.contains_key(&asset_id) {
                // insert into local db
                let po = LocalPolyMarketAssetInfoPo {
                    series_id: info.series_id.clone(),
                    series_slug: info.series_slug.clone(),
                    event_id: info.event_id.clone(),
                    event_slug: info.event_slug.clone(),
                    market_id: info.market_id.clone(),
                    market_slug: info.market_slug.clone(),
                    assert_id: asset_id.clone(),
                    assert_slug: info.asset_slug.clone(),
                };

                self.repository.create_asset(po).await?;
                res.insert(asset_id, 0u64);
            } else {
                let local_ts = *local_assets.get(&asset_id).unwrap_or(&0u64);
                if local_ts < info.latest_timestamp {
                    res.insert(asset_id, local_ts);
                }
            }
        }

        Ok(res)
    }

    ///
    /// 监听数据，写入数据库
    ///
    pub async fn start_listen(rx: mpsc::Receiver<ServerMessage>) -> Result<(), YuError> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use crate::sync::client::repository::{ClientPolyMarketRepository, MockClientPolyMarketRepositoryTrait};
    use crate::sync::client::sync_client_service::SyncClientService;
    use crate::sync::sync_server::grpc_sync::{PolymarketAssertInfo, PolyMarketAssetInfoList};
    use std::sync::Arc;
    use std::collections::HashMap;

    // helper constructor for tests to inject mock repository
    impl SyncClientService {
        fn new(repository: ClientPolyMarketRepository) -> Self {
            Self { repository }
        }
    }

    // Case 1: server has a new asset that is missing locally -> should insert and return asset_id -> 0
    #[tokio::test]
    async fn test_align_local_assets_new_asset_inserted() {
        let mut mock_repository = MockClientPolyMarketRepositoryTrait::default();

        // local DB has no assets
        mock_repository
            .expect_list_assets_timestamp()
            .returning(|| Ok(HashMap::new()));

        // expect create_asset to be called with assert_id == "a1"
        mock_repository
            .expect_create_asset()
            .withf(|po| po.assert_id == "a1")
            .times(1)
            .returning(|_po| Ok(()));

        let service = SyncClientService::new(Arc::new(mock_repository));

        let mut assets = HashMap::new();
        assets.insert(
            "a1".to_string(),
            PolymarketAssertInfo {
                series_id: "s1".to_string(),
                series_slug: "ss1".to_string(),
                event_id: "e1".to_string(),
                event_slug: "es1".to_string(),
                market_id: "m1".to_string(),
                market_slug: "ms1".to_string(),
                asset_id: "a1".to_string(),
                asset_slug: "as1".to_string(),
                latest_timestamp: 200,
            },
        );

        let server_list = PolyMarketAssetInfoList { assets };

        let res = service.align_local_assets(server_list).await.unwrap();
        assert_eq!(res.get("a1"), Some(&0u64));
    }

    // Case 2: both exist but local timestamp is older than server -> should return local timestamp
    #[tokio::test]
    async fn test_align_local_assets_outdated_local_timestamp() {
        let mut mock_repository = MockClientPolyMarketRepositoryTrait::default();

        // local DB has asset a2 with timestamp 100
        mock_repository
            .expect_list_assets_timestamp()
            .returning(|| Ok(HashMap::from([("a2".to_string(), 100u64)])));

        // no insert expected
        mock_repository.expect_create_asset().times(0);

        let service = SyncClientService::new(Arc::new(mock_repository));

        let mut assets = HashMap::new();
        assets.insert(
            "a2".to_string(),
            PolymarketAssertInfo {
                series_id: "s2".to_string(),
                series_slug: "ss2".to_string(),
                event_id: "e2".to_string(),
                event_slug: "es2".to_string(),
                market_id: "m2".to_string(),
                market_slug: "ms2".to_string(),
                asset_id: "a2".to_string(),
                asset_slug: "as2".to_string(),
                latest_timestamp: 200,
            },
        );

        let server_list = PolyMarketAssetInfoList { assets };

        let res = service.align_local_assets(server_list).await.unwrap();
        assert_eq!(res.get("a2"), Some(&100u64));
    }

    // Case 3: local has assets not present on server -> should skip and return empty map
    #[tokio::test]
    async fn test_align_local_assets_local_only_skipped() {
        let mut mock_repository = MockClientPolyMarketRepositoryTrait::default();

        // local DB has asset a3
        mock_repository
            .expect_list_assets_timestamp()
            .returning(|| Ok(HashMap::from([("a3".to_string(), 300u64)])));

        // server provides empty assets
        mock_repository.expect_create_asset().times(0);

        let service = SyncClientService::new(Arc::new(mock_repository));

        let server_list = PolyMarketAssetInfoList { assets: HashMap::new() };

        let res = service.align_local_assets(server_list).await.unwrap();
        assert!(res.is_empty());
    }
}
