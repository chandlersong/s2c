use crate::okx::duck_pos::OkxKlinePo;
use tokio::sync::broadcast;
use yue::okx::restful_api::{OKxApi, default_okx_api};

pub struct KlineService {
    pub inst_ids: Vec<String>,
    pub api: OKxApi,
    pub sender: broadcast::Sender<OkxKlinePo>,
}

impl KlineService {
    pub fn new(inst_ids: Vec<String>) -> Self {
        let (sender, _) = broadcast::channel(10000);
        Self {
            inst_ids,
            api: default_okx_api(),
            sender,
        }
    }

    pub fn initial_candle(&self) {
        todo!()
    }
}
