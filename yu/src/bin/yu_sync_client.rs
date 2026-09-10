use li::tools::logs::{parse_level, setup_logger};
use li::tools::time::unix_2_readable;
use log::{LevelFilter, debug, error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::mpsc::Sender;
use tokio_stream::StreamExt;
use tonic::Request;
use yu::config::get_config;
use yu::cron_job;
use yu::data_integrity::check::{SyncClientBinarySearchDataImpl, binary_search_gap};
use yu::data_integrity::models::ValidationGap;
use yu::errors::YuError;
use yu::postgresql_db::{PostgresqlTableTrait, get_sync_client_pg_pool};
use yu::sync::client::database::initial_grpc_client_tables;
use yu::sync::client::db_consts::ClientsTables;
use yu::sync::client::sync_client_service::{GrpcChannelManager, SyncClientService};
use yu::sync::models::grpc_sync::sync_interface_client::SyncInterfaceClient;
use yu::sync::models::grpc_sync::{Empty, Exchange, ServerMessage, SubscribeRequest, SyncRequest, instrument};
use yue::http_client::init_http_client;
use yue::models::HistoryInterval;
use yue::tools::get_snow_flake_id_u64;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let app_config = get_config();
    let proxy = app_config.proxy_url.clone();
    if let Some(url_proxy) = proxy {
        info!("Using proxy: {}", url_proxy);
        init_http_client(Some(&url_proxy));
    } else {
        init_http_client(None);
    }
    let mut special_log = HashMap::new();
    let log_in_config = app_config.log_level.as_deref();
    special_log.insert("yu_sync_client".to_string(), parse_level(log_in_config));
    special_log.insert("yu".to_string(), parse_level(log_in_config));
    special_log.insert("yue".to_string(), parse_level(log_in_config));
    special_log.insert("li".to_string(), parse_level(log_in_config));
    setup_logger(Some(LevelFilter::Warn), special_log)?;
    initial_grpc_client_tables(None).await?;

    let client_service = Arc::new(SyncClientService::default());

    let sync_client_config = match &app_config.sync_client {
        None => {
            error!("No sync client config provide provided");
            return Err(YuError::new("sync_client 配置未找到，请在配置文件中添加 sync_client 配置"));
        }
        Some(config) => config,
    };
    // //FUTURE:改成https
    let server_url = format!("http://{}:{}", sync_client_config.server_host, sync_client_config.server_port);
    info!("连接到远程服务器:{}", server_url);
    // // 连接到 gRPC 服务（根据需要修改地址）-
    let connection_manager = Arc::new(GrpcChannelManager::new(server_url.as_ref()));

    info!("已连接到 gRPC 服务端");
    //
    // // 1) 调用 GetLatestTimestamps

    let tx = match client_service.start_batch_insert(None, None).await {
        Ok(sender) => sender,
        Err(e) => {
            error!("error starting batch insert: {}", e);
            return Err(e);
        }
    };

    let subscribe_server_tx = tx.clone();
    let subscribe_server_connection = connection_manager.clone();
    tokio::spawn(async move {
        if let Err(e) = subscribe(subscribe_server_tx, subscribe_server_connection).await {
            error!("Error in subscribe: {}", e);
        }
    });

    let sync_server_tx = tx.clone();
    let sync_server_manager = connection_manager.clone();
    let sync_client_service = client_service.clone();
    tokio::spawn(async move {
        if let Err(e) = initial_data(sync_client_service, sync_server_tx, sync_server_manager).await {
            error!("Error when initial instruments with server: {}", e);
        }
    });

    let daily_sync_tx = tx.clone();
    let daily_sync_manager = connection_manager.clone();
    let daily_sync_client_service = client_service.clone();

    let _ = cron_job!("0 28 */6 * * *", move |_uuid, _locked| {
        let sync_tx = daily_sync_tx.clone();
        let sync_manager = daily_sync_manager.clone();
        let sync_client_service = daily_sync_client_service.clone();
        Box::pin(async move {
            info!("start refresh binance exchange info");
            if let Err(e) = async_sync_server(sync_client_service, sync_tx, sync_manager).await {
                error!("Error when async instruments with server: {}", e);
            }
        })
    });

    signal::ctrl_c().await.expect("监听 Ctrl+C 失败");
    Ok(())
}

async fn forward_server_stream(mut stream: tonic::Streaming<ServerMessage>, local_db_tx: Sender<ServerMessage>) -> Result<(), YuError> {
    while let Some(item) = stream.next().await {
        match item {
            Ok(server_message) => {
                if let Err(e) = local_db_tx.send(server_message).await {
                    error!("error sending server message: {}", e);
                }
            }
            Err(e) => {
                error!("error receiving server message: {}", e);
            }
        }
    }

    Ok(())
}

async fn subscribe(local_db_tx: Sender<ServerMessage>, manager: Arc<GrpcChannelManager>) -> Result<(), YuError> {
    loop {
        let connection = manager.connect().await;
        let mut server = SyncInterfaceClient::new(connection);
        //FUTURE：把这个identify改成配置文件的参数
        let id = get_snow_flake_id_u64();
        info!("subscribe_latest called with assigned id: {}", id);
        let request = SubscribeRequest { client_id: id };
        let stream = server.subscribe_latest(Request::new(request)).await?.into_inner();
        if let Err(e) = forward_server_stream(stream, local_db_tx.clone()).await {
            error!("error forwarding server stream: {}", e);
            manager.reconnect().await;
        }
    }
}

///
/// 这些信息并不是全部需要长连接的。所以暂时先不考虑锻炼身体
///
/// FUTURE:
/// 1. 可能会有一些已经关闭的instrument。这里也要同步。但是因为这里的问题其实希望client有完整信息，所以就过了吧。
async fn initial_data(
    client_service: Arc<SyncClientService>,
    local_db_tx: Sender<ServerMessage>,
    manager: Arc<GrpcChannelManager>,
) -> Result<(), YuError> {
    let mut server = SyncInterfaceClient::new(manager.connect().await);
    let resp = server.list_instrument(Request::new(Empty {})).await?;
    let inst_list = resp.into_inner();
    info!("获取instrument列表个数.{}", inst_list.instruments.len());
    let diff_from_server = client_service.align_local_instrument(inst_list).await?;
    info!("align_local_instrument done.");
    info!("需要同步polymarket的instrument个数{}", diff_from_server.polymarket_diff.len());
    info!("需要同步polymarket的oxk option个数{}", diff_from_server.okx_option_diff.len());
    info!("开始同步polymarket历史数据到本地数据库");
    let one_hour_ms = HistoryInterval::OneHour.to_milliseconds();
    for (inst_id, (start_ms, end_ms)) in diff_from_server.polymarket_diff {
        //FUTURE: 因为这里时candle begin。所以要减去一个周期，以后重构的时候，通过instrument把数据的周期也传过来。然后在这里做减去周期的操作
        if (end_ms - start_ms) < one_hour_ms {
            continue;
        }
        let stream = server
            .sync_history(Request::new(SyncRequest {
                inst_id,
                start_ms: start_ms + 1,
                end_ms,
                exchange: Exchange::Polymarket.into(),
            }))
            .await?
            .into_inner();
        forward_server_stream(stream, local_db_tx.clone()).await?;
    }
    info!("开始同步okx历史数据到本地数据库");
    for (inst_id, (start_ms, end_ms)) in diff_from_server.okx_option_diff {
        //FUTURE: 因为这里时candle begin。所以要减去一个周期，以后重构的时候，通过instrument把数据的周期也传过来。然后在这里做减去周期的操作

        if (end_ms - start_ms) < one_hour_ms {
            continue;
        }
        let end_ms = end_ms - HistoryInterval::OneHour.to_milliseconds();
        let stream = server
            .sync_history(Request::new(SyncRequest {
                inst_id,
                start_ms: start_ms + 1,
                end_ms,
                exchange: Exchange::Okx.into(),
            }))
            .await?
            .into_inner();
        forward_server_stream(stream, local_db_tx.clone()).await?;
    }
    info!("async_sync_server done. 历史数据异步写入，可能过会儿更新");
    Ok(())
}

///
/// # 说明
/// 1. instrument列表，以Sever端为准。主要是为了方便扩展。因为很多信息，比如这个instrument是否在交易等，都是在服务器端的。
///
async fn async_sync_server(
    client_service: Arc<SyncClientService>,
    local_db_tx: Sender<ServerMessage>,
    manager: Arc<GrpcChannelManager>,
) -> Result<(), YuError> {
    let mut server = SyncInterfaceClient::new(manager.connect().await);
    let resp = server.list_instrument(Request::new(Empty {})).await?;
    let inst_list = resp.into_inner();
    let pg_pool = get_sync_client_pg_pool().await?;
    let okx_binary_search_ds = SyncClientBinarySearchDataImpl::new(
        pg_pool.clone(),
        ClientsTables::OkxPriceHistory.table_name(),
        ClientsTables::OkxInstruments.table_name(),
        "candle_begin_time",
    );
    let pm_binary_search_ds = SyncClientBinarySearchDataImpl::new(
        pg_pool.clone(),
        ClientsTables::PolymarketPriceHistory.table_name(),
        ClientsTables::PolyMarketInstruments.table_name(),
        "timestamp",
    );
    let interval = HistoryInterval::OneHour; //这里存粹是因为hard code
    let now = interval.get_now_close_unix_ms_utc();
    for (_, inst) in inst_list.instruments.into_iter() {
        if let Some(payload) = inst.payload {
            match payload {
                instrument::Payload::Okx(okx_inst) => {
                    let start = interval.get_close_unix_ms(okx_inst.list_time) + interval.to_milliseconds();
                    let server_id = okx_inst.server_id.clone();
                    let gaps: Vec<ValidationGap> = binary_search_gap(
                        server_id.to_string().as_str(),
                        "SPOT",
                        start,
                        now,
                        &interval,
                        okx_binary_search_ds.clone(),
                    )?;
                    debug!("okx {} gaps: {:?}", okx_inst.inst_id, gaps.len());
                    for gap in gaps {
                        match gap {
                            ValidationGap::MissingData { start_time, end_time, .. } => {
                                debug!(
                                    "okx {} MissingData from: {} to {}",
                                    okx_inst.inst_id,
                                    unix_2_readable(&start_time),
                                    unix_2_readable(&end_time)
                                );
                                let stream = server
                                    .sync_history(Request::new(SyncRequest {
                                        inst_id: server_id,
                                        start_ms: start_time,
                                        end_ms: end_time,
                                        exchange: Exchange::Okx.into(),
                                    }))
                                    .await?
                                    .into_inner();
                                forward_server_stream(stream, local_db_tx.clone()).await?;
                            }
                            _ => {
                                error!("should not happen, gap: {:?}", gap);
                            }
                        }
                    }
                }
                instrument::Payload::Polymarket(polymarket_inst) => {
                    let start = interval.get_close_unix_ms(polymarket_inst.start_ms) + interval.to_milliseconds();
                    let server_id = polymarket_inst.server_id.clone();
                    let gaps: Vec<ValidationGap> =
                        binary_search_gap(server_id.to_string().as_str(), "SPOT", start, now, &interval, pm_binary_search_ds.clone())?;
                    debug!("polymarket {} gaps: {:?}", polymarket_inst.server_id, gaps.len());
                    for gap in gaps {
                        match gap {
                            ValidationGap::MissingData { start_time, end_time, .. } => {
                                debug!(
                                    "polymarket {} MissingData from: {} to {}",
                                    server_id,
                                    unix_2_readable(&start_time),
                                    unix_2_readable(&end_time)
                                );
                                let stream = server
                                    .sync_history(Request::new(SyncRequest {
                                        inst_id: server_id,
                                        start_ms: start_time,
                                        end_ms: end_time,
                                        exchange: Exchange::Polymarket.into(),
                                    }))
                                    .await?
                                    .into_inner();
                                forward_server_stream(stream, local_db_tx.clone()).await?;
                            }
                            _ => {
                                error!("should not happen, gap: {:?}", gap);
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
