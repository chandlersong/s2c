use li::errors::LiError;
use thiserror::Error;
use yue::errors::YueError;

#[derive(Error, Debug)]
pub enum YuError {
    #[error("system io: {0}")]
    SystemIOError(#[from] std::io::Error),
    #[error("yue error: {0}")]
    YueError(#[from] YueError),
    #[error("duckDB error: {0}")]
    DuckDBError(#[from] duckdb::Error),
    #[error("r2d2 error: {0}")]
    R2D2Error(#[from] r2d2::Error),
    #[error("li error: {0}")]
    LiError(#[from] LiError),
    #[error("parse json error: {0}")]
    SerdeJsonError(#[from] serde_json::Error),
    #[error("Not support: {0}")]
    NotSupportError(String),
    #[error("tonic::transport: {0}")]
    TonicTransportError(#[from] tonic::transport::Error),
    #[error("tonic::Status: {0}")]
    TonicTStatusError(#[from] tonic::Status),
    #[error("Custom error: {0}")]
    CustomError(String),
}

impl YuError {
    pub fn new(message: &str) -> Self {
        YuError::CustomError(message.to_string())
    }
}
