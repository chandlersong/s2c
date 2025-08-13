use aws_sdk_dynamodb::config::{Credentials, Region};
use std::env;

pub async fn create_s3_client() -> aws_sdk_s3::Client {
    let is_local = env::var("IS_LOCAL")
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(false);
    if is_local {
        let region = Region::new("us-east-1");
        let credentials =
            Credentials::new("fakeAccessKey", "fakeSecretKey", None, None, "localstack");
        let config = aws_sdk_s3::config::Builder::new()
            .region(region)
            .credentials_provider(credentials)
            .endpoint_url("http://localhost:4566")
            .build();
        aws_sdk_s3::Client::from_conf(config)
    } else {
        let config = aws_config::load_from_env().await;
        aws_sdk_s3::Client::new(&config)
    }
}
