use reqwest::{Client, Method};

#[cfg(test)]
mod tests {
    use super::*;
    use yue::http_client::{NonAuthRequestBuilder, YueRequestBuilder};
    use yue::models::RequestInfo;

    #[test]
    fn test_noauth_request_builder_with_valid_param() {
        let client = Client::new();
        let builder = NonAuthRequestBuilder {};
        let info = RequestInfo::new_full_url("https://example.com/api", false, 1, None, None).unwrap();
        let param = Some("key=value".to_string());
        let method = Method::GET;

        let result = builder.compose_request(&client, &info, param, method);
        assert!(result.is_ok());

        let request = result.unwrap().build().unwrap();
        assert_eq!(request.url().as_str(), "https://example.com/api?key=value");
    }

    #[test]
    fn test_noauth_request_builder_with_empty_param() {
        let client = Client::new();
        let builder = NonAuthRequestBuilder {};
        let info = RequestInfo::new_full_url("https://example.com/api", false, 1, None, None).unwrap();
        let param = Some("".to_string());
        let method = Method::GET;

        let result = builder.compose_request(&client, &info, param, method);
        assert!(result.is_ok());

        let request = result.unwrap().build().unwrap();
        assert_eq!(request.url().as_str(), "https://example.com/api");
    }

    #[test]
    fn test_noauth_request_builder_with_none_param() {
        let client = Client::new();
        let builder = NonAuthRequestBuilder {};
        let info = RequestInfo::new_full_url("https://example.com/api", false, 1, None, None).unwrap();
        let param = None;
        let method = Method::GET;

        let result = builder.compose_request(&client, &info, param, method);
        assert!(result.is_ok());

        let request = result.unwrap().build().unwrap();
        assert_eq!(request.url().as_str(), "https://example.com/api");
    }

    #[test]
    fn test_noauth_request_builder_with_post_method() {
        let client = Client::new();
        let builder = NonAuthRequestBuilder {};
        let info = RequestInfo::new_full_url("https://example.com/api", false, 1, None, None).unwrap();
        let param = Some("key=value".to_string());
        let method = Method::POST;

        let result = builder.compose_request(&client, &info, param, method);
        assert!(result.is_ok());

        let request = result.unwrap().build().unwrap();
        assert_eq!(request.url().as_str(), "https://example.com/api?key=value");
        assert_eq!(request.method(), Method::POST);
    }
}
