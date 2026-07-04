use sqlx::FromRow;

#[derive(Debug, FromRow)]
pub struct LocalPolyMarketAssetInfoPo {
    pub series_id: String,
    pub series_slug: String,
    pub event_id: String,
    pub event_slug: String,
    pub market_id: String,
    pub market_slug: String,
    pub assert_id: String,
    pub assert_slug: String,
}
