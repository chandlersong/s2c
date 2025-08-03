mod settings;
use log::LevelFilter;
use maester::tools::logs::setup_logger;

#[tokio::main]
async fn main() {

    let _ = setup_logger(Some(LevelFilter::Debug));

}
