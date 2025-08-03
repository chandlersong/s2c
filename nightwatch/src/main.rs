use log::LevelFilter;
use maester::tools::logs::setup_logger;
mod errors;


#[tokio::main]
async fn main() {
    let _ = setup_logger(Some(LevelFilter::Info));

}
