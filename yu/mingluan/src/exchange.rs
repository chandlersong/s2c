///
/// 主要处理各个交易所的数据的更新操作，
/// 不保存任何交易所的具体操作
///
pub(crate) trait KlineUpdate {
    /// 刷新现货K线
    async fn update(&self) -> String;
}
