use braavos::accounts::AccountReader;
use braavos::binance::bn_restful_commands::PMAccountReader;
use braavos::settings::BRAAVOS_SETTING;

#[tokio::main]
async fn main() {
    let setting = &BRAAVOS_SETTING;
    let account = setting.get_account(0);
    let account_query = account.clone();
    let reader = PMAccountReader { account: account_query };

    let account_info = reader.account_balance().unwrap();

    println!("account equity:{:?}", account_info.account_equity);
    println!("swap balance:{:?}", account_info.um_swap_summary.balance);
}