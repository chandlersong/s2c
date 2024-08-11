pub type UnixTimeStamp = u64;

//不太确定哪个好，就先用这个用于高精度计算
pub type Decimal = rust_decimal::Decimal;




pub struct AccountBalance {
    pub swap: Vec<SwapBalance>,
}


pub struct SwapBalance {
    pub symbol: String,
    pub position: Decimal,
    pub cost_price: Decimal,
    pub unrealized_profit: Decimal,
    pub price: Decimal,
}


