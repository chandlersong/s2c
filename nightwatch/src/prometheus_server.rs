use prometheus::{Encoder, Gauge, Registry, TextEncoder};

pub struct PrometheusServer {
    symbols_gauges: Vec<Gauge>,
    pub format_type: String,
}

impl PrometheusServer {
    pub fn new() -> PrometheusServer {
        PrometheusServer {
            symbols_gauges: vec![],
            format_type:String::from(TextEncoder::new().format_type())
        }
    }

    pub fn extend_gauges(&mut self, gauge: Vec<Gauge>) {
        self.symbols_gauges.extend(gauge);
    }

    pub fn print_metric(&mut self) -> Vec<u8> {
        let r = Registry::new();
        for g in &self.symbols_gauges {
            r.register(Box::new(g.clone())).unwrap()
        }

        let encoder = TextEncoder::new();
        let mut buffer = vec![];
        encoder.encode(&r.gather(), &mut buffer).unwrap();
        self.symbols_gauges.clear();
        buffer
    }
}

