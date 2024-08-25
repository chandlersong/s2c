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
    ($name:expr, $value:expr) => {
        {
            use rust_decimal::prelude::ToPrimitive;
            let res = prometheus::Gauge::with_opts(prometheus::Opts::new(&$name,format!("{}_help", $name))).unwrap();
            res.set($value.to_f64().unwrap());
            res
        }
    };
    ($name:expr, $value:expr,$(($field:expr => $field_value:expr)),+) => {{
         use rust_decimal::prelude::ToPrimitive;
         let mut ops = prometheus::Opts::new(&$name,format!("{}_help", $name));
        $(
            print!("{}",$field);
            ops = ops.clone().const_label($field,$field_value);
         )*
         let res = prometheus::Gauge::with_opts(ops).unwrap();
         res.set($value.to_f64().unwrap());
         res
    }};
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

    #[test]
    fn test_prometheus_gauge_with_map() {
        let name = "test".to_string();
        let expected = name.clone();
        let value = dec!(123.123);
        let actual: Gauge = prometheus_gauge!(name,value,("apple" => "3"),("bb" => "5"));
        println!("{:?}", actual);
        let desc = actual.desc()[0];
        assert_eq!(expected, desc.fq_name, "gauge名字错误");
        let paris = &desc.const_label_pairs;
        assert_eq!(2, paris.len(), "gauge名字错误");
        let pair = paris[0].clone();
        assert_eq!("apple", pair.get_name());
        assert_eq!("3", pair.get_value());
        assert_eq!(123.123, actual.get(), "gauge数值错误");
    }
}
