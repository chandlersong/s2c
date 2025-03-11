use crate::settings::VARYS_CONFIG;
use log::info;
use maester::notification::telegrams::OpsBot;
use std::sync::LazyLock;

pub static OPS_ROBOTS: LazyLock<OpsBot> = LazyLock::new(
    || {
        let config = &VARYS_CONFIG.robot.ops;
        info!("initializing ops robots, chat id {}",config.chat_id);
        OpsBot::new(&config.client_id, config.chat_id)
    }
);
