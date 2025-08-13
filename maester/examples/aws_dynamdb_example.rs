use aws_sdk_dynamodb::types::AttributeValue;
use maester::aws::dynamodb::create_dynamodb_client;
use std::collections::HashMap;

#[tokio::main]
async fn main() {
    // 初始化 DynamoDB 客户端
    let client = create_dynamodb_client(true).await;

    // 定义要写入的数据
    let mut item: HashMap<String, AttributeValue> = HashMap::new();
    item.insert(
        "user_id".to_string(),
        AttributeValue::S("12345".to_string()),
    );
    item.insert("name".to_string(), AttributeValue::S("张伟".to_string()));
    item.insert(
        "email".to_string(),
        AttributeValue::S("zhangwei@example.com".to_string()),
    );

    // 构建 PutItem 请求
    let request = client
        .put_item()
        .table_name("Users") // 替换为你的表名
        .set_item(Some(item));

    // 执行写入操作
    match request.send().await {
        Ok(output) => {
            println!("成功写入数据到 DynamoDB: {:?}", output);
        }
        Err(error) => {
            eprintln!("写入 DynamoDB 失败: {:?}", error);
        }
    }
}
