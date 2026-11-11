use log::SetLoggerError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LiError {
    #[error("Serde error: {0}")]
    SerdeError(#[from] serde_json::Error),
    #[error("SetLoggerError error: {0}")]
    SetLoggerError(#[from] SetLoggerError),
    #[error("Custom error: {0}")]
    CustomError(String),
}

impl LiError {
    pub fn new(message: &str) -> Self {
        LiError::CustomError(String::from(message))
    }
}
