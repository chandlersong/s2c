use hmac::digest::InvalidLength;
use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub struct YueError {
    message: String,
}

// 实现 Display trait，用于将错误信息格式化为字符串
impl fmt::Display for YueError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Custom Error: {}", self.message)
    }
}

impl YueError {
    pub fn new(message: &str) -> YueError {
        YueError {
            message: message.to_string(),
        }
    }
}

// 实现 Error trait，用于提供错误信息
impl Error for YueError {}

impl From<ureq::Error> for YueError {
    fn from(error: ureq::Error) -> Self {
        YueError {
            message: format!("request Error: {}", error),
        }
    }
}

impl From<std::io::Error> for YueError {
    fn from(error: std::io::Error) -> Self {
        YueError {
            message: format!("request Error: {}", error),
        }
    }
}

impl From<InvalidLength> for YueError {
    fn from(err: InvalidLength) -> Self {
        YueError::new(&format!("Invalid key length for HMAC: {}", err))
    }
}
