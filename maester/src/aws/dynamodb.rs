use aws_sdk_dynamodb::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_dynamodb::{Client, Config};

pub async fn create_dynamodb_client(is_local: bool) -> Client {
    // 检查环境变量 IS_LOCAL 是否为 "true" 来决定使用本地还是远程 DynamoDB

    if is_local {
        // LocalStack 配置
        let region = Region::new("us-east-1"); // LocalStack 通常使用默认区域
        let credentials = Credentials::new(
            "fakeAccessKey", // LocalStack 不验证凭证，随意填写
            "fakeSecretKey",
            None,
            None,
            "localstack",
        );
        let config = Config::builder()
            .behavior_version(BehaviorVersion::latest()) // 关键：设置行为版本
            .region(region)
            .credentials_provider(credentials)
            .endpoint_url("http://localhost:8000") // LocalStack 的默认端点
            .build();
        Client::from_conf(config)
    } else {
        // 远程 AWS DynamoDB 配置
        let config = aws_config::load_from_env().await;
        Client::new(&config)
    }
}
