use hmac::digest::InvalidLength;
use thiserror::Error;

// 将外部不可克隆的 error 类型转换为 String 存储，以便整个枚举可派生 Clone
#[derive(Error, Debug, Clone)]
pub enum YueError {
    #[error("Request error: code={code:?}, body={body:?}")]
    ExchangeRequestError { code: u16, body: String },
    #[error("Serialization/Deserialization error: {0}")]
    SerdeError(String),
    #[error("error encode: {0}")]
    Ed25519DalekError(String),
    #[error("Reqwest error: {0}")]
    RequestError(String),
    #[error("IO error: {0}")]
    IoError(String),
    #[error("Invalid key length for HMAC: {0}")]
    InvalidKeyLength(String),
    #[error("parse data error: {0}")]
    ParseError(String),
    #[error("{0}")]
    NotImplemented(String),
    #[error("{0}")]
    Timeout(String),
    #[error("Custom error: {0}")]
    CustomError(String),
}

impl YueError {
    pub fn new(message: &str) -> YueError {
        YueError::CustomError(message.to_string())
    }
}

// 手动实现从具体错误类型转换为 YueError 并保留原始错误信息的字符串表示
impl From<serde_json::Error> for YueError {
    fn from(e: serde_json::Error) -> Self {
        YueError::SerdeError(e.to_string())
    }
}

impl From<ed25519_dalek::pkcs8::Error> for YueError {
    fn from(e: ed25519_dalek::pkcs8::Error) -> Self {
        YueError::Ed25519DalekError(e.to_string())
    }
}

impl From<reqwest::Error> for YueError {
    fn from(e: reqwest::Error) -> Self {
        YueError::RequestError(e.to_string())
    }
}

impl From<std::io::Error> for YueError {
    fn from(e: std::io::Error) -> Self {
        YueError::IoError(e.to_string())
    }
}

impl From<InvalidLength> for YueError {
    fn from(e: InvalidLength) -> Self {
        YueError::InvalidKeyLength(e.to_string())
    }
}
