use braavos::models::{AccountSummary, Decimal, SwapPosition, SwapSummary};
use prometheus::{Encoder, Gauge, Registry, TextEncoder};
use rust_decimal_macros::dec;

pub trait ToGauge {
    fn to_prometheus_gauge(&self, strategy: &str) -> Vec<Gauge>;
}

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

impl ToGauge for AccountSummary {
    fn to_prometheus_gauge(&self, strategy: &str) -> Vec<Gauge> {
        let side_name = format!("{}_acc_detail", strategy);
        let acc_equity = prometheus_gauge!(side_name,self.account_equity,("field" => "acc_equity"));
        let negative_balance = prometheus_gauge!(side_name,self.negative_balance,("field" => "negative_balance"));
        let usdt_equity = prometheus_gauge!(side_name,self.usdt_equity,("field" => "usdt_equity"));
        let account_pnl = prometheus_gauge!(side_name,self.account_pnl,("field" => "account_pnl"));
        vec![acc_equity, negative_balance, usdt_equity, account_pnl]
    }
}

impl ToGauge for SwapSummary {
    fn to_prometheus_gauge(&self, strategy: &str) -> Vec<Gauge> {
        let acc = prometheus_gauge!(format!("{}_acc", strategy),self.balance);
        let pnl = prometheus_gauge!(format!("{}_pnl", strategy),self.pnl);
        let acc_long = prometheus_gauge!(format!("{}_acc_long", strategy),self.long_balance);
        let acc_long_pnl = prometheus_gauge!(format!("{}_acc_long_pnl", strategy),self.long_pnl);
        let acc_short = prometheus_gauge!(format!("{}_acc_short", strategy),self.short_balance);
        let acc_short_pnl = prometheus_gauge!(format!("{}_acc_short_pnl", strategy),self.short_pnl);
        let fra_pnl = prometheus_gauge!(format!("{}_fra_pnl", strategy),self.fra_pnl);
        vec![acc, acc_long, acc_long_pnl, acc_short, acc_short_pnl, pnl, fra_pnl]
    }
}

impl ToGauge for SwapPosition {
    fn to_prometheus_gauge(&self, strategy: &str) -> Vec<Gauge> {
        let side = if self.position_amt > dec!(0) { dec!(1) } else { dec!(-1) };
        let side_name = if side == dec!(1) { format!("{strategy}_long") } else { format!("{strategy}_short") };

        let cur_price = prometheus_gauge!(side_name,self.cur_price,("field" => "cur_price"),("symbol" => &self.symbol));
        let avg_price = prometheus_gauge!(side_name,self.avg_price,("field" => "avg_price"),("symbol" => &self.symbol));
        let pos = prometheus_gauge!(side_name,self.position_amt,("field" => "pos"),("symbol" => &self.symbol));
        let pnl_u = prometheus_gauge!(side_name,self.pnl_u,("field" => "pnl_u"),("symbol" => &self.symbol));
        let value = prometheus_gauge!(side_name,self.pos_u,("field" => "value"),("symbol" => &self.symbol));


        let change_value: Decimal = (self.cur_price / self.avg_price - dec!(1)) * side;
        let change = prometheus_gauge!(side_name,change_value,("field" => "change"),("symbol" => &self.symbol));
        vec![cur_price, pos, pnl_u, avg_price, change, value]
    }
}



#[cfg(test)]
mod tests {
    use crate::prometheus_server::ToGauge;
    use braavos::models::{AccountSummary, SwapPosition, SwapSummary};
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

    #[test]
    fn test_to_swap_position_prometheus() {
        let swap_position = SwapPosition {
            symbol: "bbb".to_string(),
            cur_price: dec!(1),
            avg_price: dec!(2),
            pos_u: Default::default(),
            pnl_u: Default::default(),
            position_amt: Default::default(),
        };
        let actual = swap_position.to_prometheus_gauge("test");

        assert_eq!(6, actual.len());
    }

    #[test]
    fn test_to_swap_summary_prometheus() {
        let swap_position = SwapSummary {
            long_balance: Default::default(),
            long_pnl: Default::default(),
            short_balance: Default::default(),
            short_pnl: Default::default(),
            balance: Default::default(),
            pnl: Default::default(),
            fra_pnl: Default::default(),
            positions: vec![],
        };
        let actual = swap_position.to_prometheus_gauge("test");

        assert_eq!(7, actual.len());
    }

    #[test]
    fn test_to_account_summary_prometheus() {
        let swap_position = AccountSummary {
            usdt_equity: Default::default(),
            negative_balance: Default::default(),
            account_pnl: Default::default(),
            account_equity: Default::default(),
            um_swap_summary: SwapSummary {
                long_balance: Default::default(),
                long_pnl: Default::default(),
                short_balance: Default::default(),
                short_pnl: Default::default(),
                balance: Default::default(),
                pnl: Default::default(),
                fra_pnl: Default::default(),
                positions: vec![],
            },
        };
        let actual = swap_position.to_prometheus_gauge("test");

        assert_eq!(4, actual.len());
    }

}
