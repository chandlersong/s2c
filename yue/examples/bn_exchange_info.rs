use std::error::Error;
use std::fs::File;
use std::io::Write;
use yue::binance::spots::get_trading_spot_symbols;
use yue::http_client::init_http_client;

/// 获取所有币安现货交易对信息并保存到CSV文件
///
/// 这个例子演示了如何调用 get_trading_spot_symbols 函数
/// 获取所有交易对的信息（包括各种状态的交易对）
/// 并将结果保存为CSV格式的文件
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
     let proxy = Option::from("http://localhost:7891");
    // 初始化HTTP客户端
    init_http_client(proxy);


    println!("开始获取币安现货交易对信息...");

    // 调用API获取所有交易对信息
    let symbols = get_trading_spot_symbols(Some("ALL")).await?;

    println!("成功获取到 {} 个交易对信息", symbols.len());

    // 创建CSV文件并写入UTF-8-BOM
    let file_path = "binance_spot_symbols.csv";
    let mut file = File::create(file_path)?;
    // 写入UTF-8-BOM (0xEF, 0xBB, 0xBF) 以确保正确的字符编码识别
    file.write_all(&[0xEF, 0xBB, 0xBF])?;
    let mut writer = csv::Writer::from_writer(file);

    // 写入CSV头部
    writer.write_record(&[
        "symbol",
        "status",
        "base_asset",
        "quote_asset_precision",
        "order_types"
    ])?;

    // 写入数据行
    for symbol_info in &symbols {
        // 将order_types数组转换为逗号分隔的字符串
        let order_types_str = symbol_info.order_types.join(",");

        writer.write_record(&[
            &symbol_info.symbol,
            &symbol_info.status,
            &symbol_info.base_asset,
            &symbol_info.quote_asset_precision.to_string(),
            &order_types_str,
        ])?;
    }

    // 刷新并关闭writer
    writer.flush()?;

    println!("数据已成功保存到文件: {}", file_path);

    // 打印统计信息
    let trading_count = symbols.iter().filter(|s| s.status == "TRADING").count();
    let halt_count = symbols.iter().filter(|s| s.status == "HALT").count();
    let break_count = symbols.iter().filter(|s| s.status == "BREAK").count();
    let end_of_day_count = symbols.iter().filter(|s| s.status == "END_OF_DAY").count();

    println!("统计信息:");
    println!("  交易中 (TRADING): {}", trading_count);
    println!("  暂停交易 (HALT): {}", halt_count);
    println!("  休市 (BREAK): {}", break_count);
    println!("  收盘 (END_OF_DAY): {}", end_of_day_count);
    println!("  其他状态: {}", symbols.len() - trading_count - halt_count - break_count - end_of_day_count);

    // 显示前5个交易对作为示例
    println!("\n前5个交易对示例:");
    for (i, symbol) in symbols.iter().take(5).enumerate() {
        println!("  {}. {} ({}) - 基础资产: {}, 报价精度: {}",
                 i + 1,
                 symbol.symbol,
                 symbol.status,
                 symbol.base_asset,
                 symbol.quote_asset_precision);
    }

    Ok(())
}
