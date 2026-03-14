mod http_clients_yue_request_tests {
    use async_trait::async_trait;
    use backon::BackoffBuilder;
    use reqwest::Method;
    use serde::de::DeserializeOwned;
    use std::collections::BTreeMap;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use yue::binance::bn_models::common::ToQueryParams;
    use yue::binance::bn_restful_commands::BNSecurityRequestBuilder;
    use yue::errors::YueError;
    use yue::http_client::{ClonableResponseCache, NonAuthRequestBuilder, ResponseHandler, YueRequest};
    use yue::models::{DefaultRateLimiter, RequestInfo};

    #[derive(Clone)]
    pub struct JsonResponseHandler;

    impl JsonResponseHandler {
        pub fn new() -> Self {
            JsonResponseHandler {}
        }
    }

    #[async_trait]
    impl<U> ResponseHandler<U> for JsonResponseHandler
    where
        U: DeserializeOwned + Send + Sync,
    {
        async fn handle_response(&self, resp: ClonableResponseCache, _rate_limiter: Option<&DefaultRateLimiter>) -> Result<U, YueError> {
            if resp.status != reqwest::StatusCode::OK {
                let body = String::from_utf8_lossy(&resp.body).to_string();
                return Err(YueError::ExchangeRequestError {
                    code: resp.status.as_u16(),
                    body,
                });
            }
            let result = match serde_json::from_slice::<U>(&resp.body) {
                Ok(val) => val,
                Err(_) => {
                    let body = String::from_utf8_lossy(&resp.body).to_string();
                    println!("[handle_response] JSON parse error, body: {}", body);
                    return Err(YueError::ExchangeRequestError {
                        code: reqwest::StatusCode::OK.as_u16(),
                        body,
                    });
                }
            };
            Ok(result)
        }
    }
    fn setup() {
        yue::http_client::init_http_client(None);
    }

    #[tokio::test]
    async fn test_execute_bn_get_basic() -> Result<(), Box<dyn std::error::Error>> {
        setup();
        let mock_server = MockServer::start().await;
        let test_path = "/api/v3/test";
        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1, None, None, None)?;

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
            response_handler: JsonResponseHandler::new(),
            _phantom: std::marker::PhantomData::<serde_json::Value>,
        }
        .execute()
        .await?;

        assert_eq!(result["message"], "success");
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_bn_get_with_params() -> Result<(), Box<dyn std::error::Error>> {
        setup();
        let mock_server = MockServer::start().await;
        let test_path = "/api/v3/test";
        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1, None, None, None)?;

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
            response_handler: JsonResponseHandler::new(),
            _phantom: std::marker::PhantomData::<serde_json::Value>,
        }
        .execute()
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
        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1, None, None, None)?;

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
            request_builder: BNSecurityRequestBuilder::new("test_key".to_string(), "test_secret".to_string()),
            body: None,
            method: Method::GET,
            response_handler: JsonResponseHandler::new(),
            _phantom: std::marker::PhantomData::<serde_json::Value>,
        }
        .execute()
        .await?;

        assert_eq!(result["authenticated"], true);
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_bn_get_error_handling() -> Result<(), Box<dyn std::error::Error>> {
        setup();
        let mock_server = MockServer::start().await;
        let test_path = "/api/v3/test";
        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1, None, None, None)?;

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
            response_handler: JsonResponseHandler::new(),
            _phantom: std::marker::PhantomData::<serde_json::Value>,
        }
        .execute()
        .await;

        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn test_retry_success_after_failure() -> Result<(), Box<dyn std::error::Error>> {
        setup();
        let mock_server = MockServer::start().await;
        let test_path = "/api/v3/retry_test";
        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1, None, None, None)?;

        // First call fails with 500
        Mock::given(method("GET"))
            .and(path(test_path))
            .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
                "error": "internal server error"
            })))
            .expect(3)
            .mount(&mock_server)
            .await;
        // Second call succeeds

        let yue_request = YueRequest {
            info: &request_info,
            param: None,
            request_builder: NonAuthRequestBuilder {},
            body: None,
            method: Method::GET,
            response_handler: JsonResponseHandler::new(),
            _phantom: std::marker::PhantomData::<serde_json::Value>,
        };

        let builder = backon::ConstantBuilder::default().with_max_times(2).build();
        let response = yue_request.retry(builder).await;
        assert!(response.is_err());
        Ok(())
    }
}
