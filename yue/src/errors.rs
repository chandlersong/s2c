use hmac::digest::InvalidLength;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum YueError {
    #[error("Request error: code={code:?}, body={body:?}")]
    ExchangeRequestError { code: u16, body: String },
    #[error("Serialization/Deserialization error: {0}")]
    SerdeError(#[from] serde_json::Error),
    #[error("error encode: {0}")]
    Ed25519DalekError(#[from] ed25519_dalek::pkcs8::Error),
    #[error("Reqwest error: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Invalid key length for HMAC: {0}")]
    InvalidKeyLength(#[from] InvalidLength),
    #[error("parse data error: {0}")]
    ParseError(String),
    #[error("{0}")]
    NotImplemented(String),
    #[error("Custom error: {0}")]
    CustomError(String),
}

impl YueError {
    pub fn new(message: &str) -> YueError {
        YueError::CustomError(message.to_string())
    }
}
