use li::aws::dynamodb::{create_dynamodb_client, create_kline_table};
use li::errors::MaesterError;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct User {
    user_id: String,
    name: String,
    email: String,
}

#[tokio::main]
async fn main() -> Result<(), MaesterError> {
    // 初始化 DynamoDB 客户端
    let client = create_dynamodb_client(true).await?;

    create_kline_table(&client, "bn_kline1", None).await?;
    println!("Table 'bn_kline1' created successfully");

    // let user = User {
    //     user_id: "12345".to_string(),
    //     name: "张伟".to_string(),
    //     email: "zhangwei@example.com".to_string(),
    // };

    // // 自动转换方式（类型完全兼容）
    // let item_auto = {
    //     let item: HashMap<String, serde_dynamo::AttributeValue> =
    //         serde_dynamo::to_item(&user).unwrap();
    //     item.into_iter()
    //         .map(|(k, v)| (k, convert_attr_value(&v)))
    //         .collect::<HashMap<String, AttributeValue>>()
    // };
    // let request_auto = client
    //     .put_item()
    //     .table_name("Users")
    //     .set_item(Some(item_auto));
    // match request_auto.send().await {
    //     Ok(output) => {
    //         println!("自动转换方式成功写入数据到 DynamoDB: {:?}", output);
    //     }
    //     Err(e) => {
    //         println!("自动转换方式写入失败: {:?}", e);
    //     }
    // }

    // // 手动转换方式
    // let item_manual = user.to_item();
    // let request_manual = client
    //     .put_item()
    //     .table_name("Users")
    //     .set_item(Some(item_manual));
    // match request_manual.send().await {
    //     Ok(output) => {
    //         println!("手动转换方式成功写入数据到 DynamoDB: {:?}", output);
    //     }
    //     Err(e) => {
    //         println!("手动转换方式写入失败: {:?}", e);
    //     }
    // }

    Ok(())
}
