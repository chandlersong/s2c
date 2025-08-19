use aws_sdk_dynamodb::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_dynamodb::types::{
    AttributeDefinition, KeySchemaElement, KeyType, ProvisionedThroughput, ScalarAttributeType,
};
use aws_sdk_dynamodb::{Client, Config};

pub async fn create_dynamodb_client(is_local: bool) -> Result<Client, aws_sdk_dynamodb::Error> {
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
        Ok(Client::from_conf(config))
    } else {
        // 远程 AWS DynamoDB 配置
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        Ok(Client::new(&config))
    }
}

pub async fn create_kline_table(
    client: &Client,
    table_name: &str,
    provisioned_throughput: Option<ProvisionedThroughput>,
) -> Result<(), aws_sdk_dynamodb::Error> {
    // 创建表
    let pt = provisioned_throughput.unwrap_or_else(|| {
        ProvisionedThroughput::builder()
            .read_capacity_units(5)
            .write_capacity_units(5)
            .build()
            .expect("Failed to build ProvisionedThroughput")
    });
    client
        .create_table()
        .table_name(table_name)
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("timestamp")
                .attribute_type(ScalarAttributeType::N)
                .build()
                .expect("Failed to build AttributeDefinition"),
        )
        .attribute_definitions(
            AttributeDefinition::builder()
                .attribute_name("symbol")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .expect("Failed to build AttributeDefinition"),
        )
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("timestamp")
                .key_type(KeyType::Hash)
                .build()
                .expect("Failed to build KeySchemaElement"),
        )
        .key_schema(
            KeySchemaElement::builder()
                .attribute_name("symbol")
                .key_type(KeyType::Range)
                .build()
                .expect("Failed to build KeySchemaElement"),
        )
        .provisioned_throughput(pt)
        .send()
        .await?;

    // 启用 TTL
    let ttl_spec = aws_sdk_dynamodb::types::TimeToLiveSpecification::builder()
        .enabled(true)
        .attribute_name("ttl")
        .build()
        .expect("Failed to build TimeToLiveSpecification");

    client
        .update_time_to_live()
        .table_name(table_name)
        .time_to_live_specification(ttl_spec)
        .send()
        .await?;

    Ok(())
}
