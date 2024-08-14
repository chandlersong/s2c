use prometheus::{Encoder, Gauge, Opts, Registry, TextEncoder};

pub struct PrometheusServer {
    symbols_gauges: Vec<Gauge>,
    strategy: String,
    pub format_type: String,
}

impl PrometheusServer {
    pub fn new(strategy: &str) -> PrometheusServer {
        PrometheusServer {
            symbols_gauges: vec![],
            strategy: String::from(strategy),
            format_type:String::from(TextEncoder::new().format_type())
        }
    }

    pub fn add_new_symbol(&mut self, symbol: &str, property: &str, value: f64, p_value: &str) {
        let gauge_opts = Opts::new(format!("{0}_acc", &self.strategy), format!("{0}_acc_help", &self.strategy))
            .const_label("symbol", symbol)
            .const_label(property, p_value);
        let gauge = Gauge::with_opts(gauge_opts).unwrap();
        gauge.set(value);

        self.symbols_gauges.push(gauge);
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

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn test_symbol_server_normal() {
        let mut server = PrometheusServer::new("stg");
        server.add_new_symbol("coinA", "field", 1.1f64, "open");
        server.add_new_symbol("coinB", "field", 2.1f64, "close");
        let buffer = server.print_metric();
        let expect = String::from("# HELP stg_acc stg_acc_help\n# TYPE stg_acc gauge\nstg_acc{field=\"close\",symbol=\"coinB\"} 2.1\nstg_acc{field=\"open\",symbol=\"coinA\"} 1.1\n");
        let actual = String::from_utf8(buffer).unwrap();
        println!("{}", &actual);
        assert_eq!(expect, actual);
    }
}
