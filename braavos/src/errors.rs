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

impl BraavosError {
    pub fn new(message: &str) -> BraavosError {
        BraavosError {
            message: message.to_string(),
        }
    }
}

// 实现 Error trait，用于提供错误信息
impl Error for BraavosError {}

impl From<ureq::Error> for BraavosError {
    fn from(error: ureq::Error) -> Self {
        BraavosError {
            message: format!("request Error: {}", error),
        }
    }
}

impl From<std::io::Error> for BraavosError {
    fn from(error: std::io::Error) -> Self {
        BraavosError {
            message: format!("request Error: {}", error),
        }
    }
}
