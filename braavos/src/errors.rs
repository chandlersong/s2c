use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub struct BraavosError {
    message: String,
}

// 实现 Display trait，用于将错误信息格式化为字符串
impl fmt::Display for BraavosError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Custom Error: {}", self.message)
    }
}

// 实现 Error trait，用于提供错误信息
impl Error for BraavosError {}

// 实现 From trait，将 io::Error 转换为 CustomError
impl From<reqwest::Error> for BraavosError {
    fn from(error: reqwest::Error) -> Self {
        BraavosError {
            message: format!("request Error: {}", error),
        }
    }
}
