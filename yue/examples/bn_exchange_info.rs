use std::error::Error;
use std::fs::File;
use std::io::Write;
use yue::binance::history_data::{CONTRACT_TYPE_PERPETUAL, TradingSymbolInfo, get_trading_spot_symbols, get_trading_swap_symbols};
use yue::http_client::init_http_client;

/// 将symbols写入CSV文件
fn write_symbols_to_csv(file_path: &str, symbols: &[TradingSymbolInfo]) -> Result<(), Box<dyn Error>> {
    let mut file = File::create(file_path)?;
    // 写入UTF-8-BOM (0xEF, 0xBB, 0xBF)
    file.write_all(&[0xEF, 0xBB, 0xBF])?;
    let mut writer = csv::Writer::from_writer(file);
    writer.write_record(&[
        "symbol",
        "status",
        "base_asset",
        "quote_asset",
        "quote_asset_precision",
        "order_types",
        "type",
    ])?;
    for symbol_info in symbols {
        let order_types_str = symbol_info.order_types.join(",");
        writer.write_record(&[
            &symbol_info.symbol,
            &symbol_info.status,
            &symbol_info.base_asset,
            &symbol_info.quote_asset,
            &symbol_info.quote_asset_precision.to_string(),
            &order_types_str,
            &symbol_info.symbol_type,
        ])?;
    }
    writer.flush()?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let start = std::time::Instant::now();
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy);

    println!("开始获取币安现货交易对信息...");
    let spot_symbols = get_trading_spot_symbols(Some("ALL")).await?;
    println!("成功获取到 {} 个现货交易对信息", spot_symbols.len());
    write_symbols_to_csv("binance_spot_symbols.csv", &spot_symbols)?;
    println!("现货数据已保存到 binance_spot_symbols.csv");

    // 获取swap交易对信息l
    println!("开始获取币安U本位合约交易对信息...");
    let swap_symbols = get_trading_swap_symbols(Some("ALL"), Some(CONTRACT_TYPE_PERPETUAL)).await?;
    println!("成功获取到 {} 个U本位合约交易对信息", swap_symbols.len());
    write_symbols_to_csv("binance_swap_symbols.csv", &swap_symbols)?;
    println!("U本位合约数据已保存到 binance_swap_symbols.csv");

    // 统计信息输出函数
    fn print_stats(symbols: &[TradingSymbolInfo], label: &str) {
        let trading_count = symbols.iter().filter(|s| s.status == "TRADING").count();
        let halt_count = symbols.iter().filter(|s| s.status == "HALT").count();
        let break_count = symbols.iter().filter(|s| s.status == "BREAK").count();
        let end_of_day_count = symbols.iter().filter(|s| s.status == "END_OF_DAY").count();
        println!("{}统计信息:", label);
        println!("  交易中 (TRADING): {}", trading_count);
        println!("  暂停交易 (HALT): {}", halt_count);
        println!("  休市 (BREAK): {}", break_count);
        println!("  收盘 (END_OF_DAY): {}", end_of_day_count);
        println!(
            "  其他状态: {}",
            symbols.len() - trading_count - halt_count - break_count - end_of_day_count
        );
        println!("\n前5个交易对示例:");
        for (i, symbol) in symbols.iter().take(5).enumerate() {
            println!(
                "  {}. {} ({}) - 基础资产: {}, 报价资产: {}, 报价精度: {}",
                i + 1,
                symbol.symbol,
                symbol.status,
                symbol.base_asset,
                symbol.quote_asset,
                symbol.quote_asset_precision
            );
        }
    }

    print_stats(&spot_symbols, "现货");
    print_stats(&swap_symbols, "U本位合约");
    println!("运行耗时: {:?}", start.elapsed());
    Ok(())
}
