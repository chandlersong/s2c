use braavos::binance::bn_models::{BinanceBase, BinancePath, CommandInfo, ListenKeyResponse, PmAPI};
use braavos::binance::bn_restful_commands::{PostCommand, PutCommand};
use braavos::models::EmptyObject;
use braavos::settings::BRAAVOS_SETTING;
use braavos::tools::setup_logger;
use log::{error, info, LevelFilter};

///
/// 主要是对对账户的监控
///
///

#[tokio::main]
async fn main() {
    let _ = setup_logger(Some(LevelFilter::Trace));
    let setting = &BRAAVOS_SETTING;
    let account = setting.get_account(0);
    // 
    let listen_key_command_info = CommandInfo::new_with_security(BinanceBase::PortfolioMargin,
                                                                 BinancePath::PAPI(PmAPI::ListenKey),
                                                                 &account.api_key,
                                                                 &account.secret);

    let create_listen_key_command = PostCommand::<EmptyObject, ListenKeyResponse>::new();
    match create_listen_key_command.execute(listen_key_command_info, None, None).await {
        Ok(v) => {
            info!("listen key is :{}",v.listen_key);
        }
        Err(e) => {
            error!("listen key command execution failed.error is:{}",e);
        }
    }
    let refresh_listen_key_info = CommandInfo::new_with_security(BinanceBase::PortfolioMargin,
                                                                 BinancePath::PAPI(PmAPI::ListenKey),
                                                                 &account.api_key,
                                                                 &account.secret);

    let refresh_listen_key_command = PutCommand::<EmptyObject, EmptyObject>::new();
    match refresh_listen_key_command.execute(refresh_listen_key_info, None, None).await {
        Ok(_) => {
            info!("refresh listen key success");
        }
        Err(e) => {
            error!("listen key command execution failed.error is:{}",e);
        }
    }
}
