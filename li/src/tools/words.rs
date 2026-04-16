use std::borrow::Cow;
use unicode_segmentation::UnicodeSegmentation;

/// 不裁剪时返回 Borrowed；裁剪时返回 Owned(截断 + "...")
///
/// 修复: 去掉显式的生命周期参数（Rust 可以自动 elide），避免 `Parameter types contain explicit lifetimes that could be elided` 警告。
pub fn take_or_all_cow_with_ellipsis(s: &str, n: usize) -> Cow<'_, str> {
    let graphemes = UnicodeSegmentation::graphemes(s, true).collect::<Vec<&str>>();
    if graphemes.len() <= n {
        Cow::Borrowed(s)
    } else {
        let part = graphemes[..n].concat();
        Cow::Owned(format!("{}...", part))
    }
}
