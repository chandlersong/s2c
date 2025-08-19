use aws_sdk_dynamodb::types::AttributeValue;
use aws_smithy_types::Blob;
use maester::aws::dynamodb::{create_dynamodb_client, create_kline_table};
use serde::{Deserialize, Serialize};
use serde_dynamo;
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
struct User {
    user_id: String,
    name: String,
    email: String,
}

impl User {
    // 手动转换方式
    fn to_item(&self) -> HashMap<String, AttributeValue> {
        let mut item = HashMap::new();
        item.insert(
            "user_id".to_string(),
            AttributeValue::S(self.user_id.clone()),
        );
        item.insert("name".to_string(), AttributeValue::S(self.name.clone()));
        item.insert("email".to_string(), AttributeValue::S(self.email.clone()));
        item
    }
}

// 辅助函数：将 serde_dynamo::AttributeValue 转为 aws_sdk_dynamodb::types::AttributeValue
fn convert_attr_value(val: &serde_dynamo::AttributeValue) -> AttributeValue {
    match val {
        serde_dynamo::AttributeValue::S(s) => AttributeValue::S(s.clone()),
        serde_dynamo::AttributeValue::N(n) => AttributeValue::N(n.clone()),
        serde_dynamo::AttributeValue::Bool(b) => AttributeValue::Bool(*b),
        serde_dynamo::AttributeValue::Null(_) => AttributeValue::Null(true),
        serde_dynamo::AttributeValue::B(b) => AttributeValue::B(Blob::new(b.clone())),
        serde_dynamo::AttributeValue::Ss(ss) => AttributeValue::Ss(ss.clone()),
        serde_dynamo::AttributeValue::Ns(ns) => AttributeValue::Ns(ns.clone()),
        serde_dynamo::AttributeValue::Bs(bs) => {
            AttributeValue::Bs(bs.iter().map(|v| Blob::new(v.clone())).collect())
        }
        serde_dynamo::AttributeValue::L(list) => {
            AttributeValue::L(list.iter().map(convert_attr_value).collect())
        }
        serde_dynamo::AttributeValue::M(map) => AttributeValue::M(
            map.iter()
                .map(|(k, v)| (k.clone(), convert_attr_value(v)))
                .collect(),
        ),
    }
}

#[tokio::main]
async fn main() {
    // 初始化 DynamoDB 客户端
    let client = create_dynamodb_client(true)
        .await
        .expect("Failed to create DynamoDB client");

    if let Err(e) = create_kline_table(&client, "bn_kline1", None).await {
        eprintln!("Error creating table: {:?}", e);
        return;
    }
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
}
