use crate::binance::bn_models::{
    ExchangeInfo, EmptyQueryParams, TradingSymbolInfo,
};
use crate::binance::bn_restful_commands::{execute_bn_get, EXCHANGE_INFO_COMMAND};
use crate::errors::BraavosError;

/// 获取现货交易对信息
/// 
/// # 参数
/// * `status` - 交易对状态过滤器
///   - `None` 或 `Some("TRADING")`: 只返回交易中的交易对 (默认)
///   - `Some("ALL")`: 返回所有交易对 (不进行状态过滤)
///   - `Some("HALT")`: 只返回暂停交易的交易对
///   - 其他值: 按指定状态过滤
/// 
/// # 返回
/// 返回符合条件的交易对信息列表，包含 symbol, status, base_asset, quote_asset_precision, order_types
pub async fn get_trading_spot_symbols(status: Option<&str>) -> Result<Vec<TradingSymbolInfo>, BraavosError> {
    let exchange_info: ExchangeInfo = execute_bn_get::<EmptyQueryParams, ExchangeInfo>(
        &EXCHANGE_INFO_COMMAND,
        None,
        None,
    )
    .await?;

    let filter_status = status.unwrap_or("TRADING");

    let trading_symbols: Vec<TradingSymbolInfo> = if filter_status == "ALL" {
        // Return all symbols without filtering
        exchange_info
            .symbols
            .into_iter()
            .map(|symbol| TradingSymbolInfo {
                symbol: symbol.symbol,
                status: symbol.status,
                base_asset: symbol.base_asset,
                quote_asset_precision: symbol.quote_asset_precision,
                order_types: symbol.order_types,
            })
            .collect()
    } else {
        // Filter by specified status
        exchange_info
            .symbols
            .into_iter()
            .filter(|symbol| symbol.status == filter_status)
            .map(|symbol| TradingSymbolInfo {
                symbol: symbol.symbol,
                status: symbol.status,
                base_asset: symbol.base_asset,
                quote_asset_precision: symbol.quote_asset_precision,
                order_types: symbol.order_types,
            })
            .collect()
    };

    Ok(trading_symbols)
}
