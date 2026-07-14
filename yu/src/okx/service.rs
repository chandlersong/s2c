pub struct CandleService {
    pub inst_ids: Vec<String>,
}

impl CandleService {
    pub fn new(inst_ids: Vec<String>) -> Self {
        Self { inst_ids }
    }

    pub fn initial_candle(&self) {
        todo!()
    }
}
