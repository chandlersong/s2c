mod http_clients_yue_request_tests {
    use reqwest::Method;
    use std::collections::BTreeMap;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use yue::binance::bn_models::ToQueryParams;
    use yue::binance::bn_restful_commands::BNSecurityRequestBuilder;
    use yue::http_client::{NonAuthRequestBuilder, YueRequest};
    use yue::models::RequestInfo;

    fn setup() {
        yue::http_client::init_http_client(None);
    }

    #[tokio::test]
    async fn test_execute_bn_get_basic() -> Result<(), Box<dyn std::error::Error>> {
        setup();
        let mock_server = MockServer::start().await;
        let test_path = "/api/v3/test";
        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1)?;

        Mock::given(method("GET"))
            .and(path(test_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "message": "success"
            })))
            .mount(&mock_server)
            .await;

        let result: serde_json::Value = YueRequest {
            info: &request_info,
            param: None,
            request_builder: NonAuthRequestBuilder {},
            body: None,
            method: Method::GET,
            _phantom: std::marker::PhantomData::<serde_json::Value>,
        }
        .execute(None)
        .await?;

        assert_eq!(result["message"], "success");
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_bn_get_with_params() -> Result<(), Box<dyn std::error::Error>> {
        setup();
        let mock_server = MockServer::start().await;
        let test_path = "/api/v3/test";
        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1)?;

        Mock::given(method("GET"))
            .and(path(test_path))
            .and(wiremock::matchers::query_param("symbol", "BTCUSDT"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "symbol": "BTCUSDT",
                "price": "50000.00"
            })))
            .mount(&mock_server)
            .await;

        let mut params = BTreeMap::new();
        params.insert("symbol", "BTCUSDT".to_string());

        let result: serde_json::Value = YueRequest {
            info: &request_info,
            param: Some(params.to_query_string()),
            request_builder: NonAuthRequestBuilder {},
            body: None,
            method: Method::GET,
            _phantom: std::marker::PhantomData::<serde_json::Value>,
        }
        .execute(None)
        .await?;

        assert_eq!(result["symbol"], "BTCUSDT");
        assert_eq!(result["price"], "50000.00");
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_bn_get_with_security() -> Result<(), Box<dyn std::error::Error>> {
        setup();
        let mock_server = MockServer::start().await;
        let test_path = "/api/v3/test";
        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1)?;

        Mock::given(method("GET"))
            .and(path(test_path))
            .and(wiremock::matchers::header("X-MBX-APIKEY", "test_key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "authenticated": true
            })))
            .mount(&mock_server)
            .await;

        let result: serde_json::Value = YueRequest {
            info: &request_info,
            param: None,
            request_builder: BNSecurityRequestBuilder {
                api_key: "test_key".to_string(),
                api_secret: "test_secret".to_string(),
            },
            body: None,
            method: Method::GET,
            _phantom: std::marker::PhantomData::<serde_json::Value>,
        }
        .execute(None)
        .await?;

        assert_eq!(result["authenticated"], true);
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_bn_get_error_handling() -> Result<(), Box<dyn std::error::Error>> {
        setup();
        let mock_server = MockServer::start().await;
        let test_path = "/api/v3/test";
        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1)?;

        Mock::given(method("GET"))
            .and(path(test_path))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "code": -1121,
                "msg": "Invalid symbol"
            })))
            .mount(&mock_server)
            .await;

        let result = YueRequest {
            info: &request_info,
            param: None,
            request_builder: NonAuthRequestBuilder {},
            body: None,
            method: Method::GET,
            _phantom: std::marker::PhantomData::<serde_json::Value>,
        }
        .execute(None)
        .await;

        assert!(result.is_err());
        Ok(())
    }
}
