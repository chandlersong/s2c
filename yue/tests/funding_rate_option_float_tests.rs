use yue::binance::bn_models::FundingRate;

#[test]
fn test_funding_rate_mark_price_some() {
    let json = r#"{
        "symbol": "BTCUSDT",
        "fundingRate": "0.0001",
        "fundingTime": 1694102460000,
        "markPrice": "27345.12"
    }"#;
    let fr: FundingRate = serde_json::from_str(json).unwrap();
    assert_eq!(fr.mark_price, Some(27345.12));
}

#[test]
fn test_funding_rate_mark_price_empty_string() {
    let json = r#"{
        "symbol": "BTCUSDT",
        "fundingRate": "0.0001",
        "fundingTime": 1694102460000,
        "markPrice": ""
    }"#;
    let fr: FundingRate = serde_json::from_str(json).unwrap();
    assert_eq!(fr.mark_price, None);
}

#[test]
fn test_funding_rate_mark_price_null() {
    let json = r#"{
        "symbol": "BTCUSDT",
        "fundingRate": "0.0001",
        "fundingTime": 1694102460000,
        "markPrice": null
    }"#;
    let fr: FundingRate = serde_json::from_str(json).unwrap();
    assert_eq!(fr.mark_price, None);
}
