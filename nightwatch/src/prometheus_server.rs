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

#[macro_export]
macro_rules! prometheus_gauge {
    ($a:expr, $b:expr) => {
        {
            use crate::models::Decimal;
            use prometheus::Gauge;
            use prometheus::Opts;
            use rust_decimal::prelude::ToPrimitive;
            let name:String = $a;
            let value:Decimal = $b;
            let res = Gauge::with_opts(Opts::new(&name,format!("{}_help", name))).unwrap();
            res.set($b.to_f64().unwrap());
            res
        }
    };
    ($a:expr, $b:expr, $c:expr) => {
        ($a + $b) * $c
    };
}

#[cfg(test)]
mod tests {
    use prometheus::core::Collector;
    use prometheus::Gauge;
    use rust_decimal_macros::dec;

    #[test]
    fn test_prometheus_gauge() {
        let name = "test".to_string();
        let expected = name.clone();
        let value = dec!(123.123);
        let actual: Gauge = prometheus_gauge!(name,value);
        print!("{:?}", actual);
        assert_eq!(expected, actual.desc()[0].fq_name, "gauge名字错误");
        assert_eq!(123.123, actual.get(), "gauge数值错误");
    }
}
