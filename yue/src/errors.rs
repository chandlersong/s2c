use hmac::digest::InvalidLength;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum YueError {
    #[error("Request error: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Invalid key length for HMAC: {0}")]
    InvalidKeyLength(#[from] InvalidLength),
    #[error("Custom error: {0}")]
    CustomError(String),
}

impl YueError {
    pub fn new(message: &str) -> YueError {
        YueError::CustomError(message.to_string())
    }
}
