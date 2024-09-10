use std::error::Error;
use std::fmt;

// 自定义错误类型
#[derive(Debug)]
pub struct NightWatchError {
    message: String,
}

// 实现 Display trait，用于将错误信息格式化为字符串
impl fmt::Display for NightWatchError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Custom Error: {}", self.message)
    }
}

// 实现 Error trait，用于提供错误信息
impl Error for NightWatchError {}
