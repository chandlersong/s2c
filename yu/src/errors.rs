use li::errors::LiError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum YuError {
    #[error("system io: {0}")]
    SystemIOError(#[from] std::io::Error),
    #[error("yue error: {0}")]
    YueError(#[from] yue::errors::YueError),
    #[error("duckDB error: {0}")]
    DuckDBError(#[from] duckdb::Error),
    #[error("r2d2 error: {0}")]
    R2D2Error(#[from] r2d2::Error),
    #[error("li error: {0}")]
    LiError(#[from] LiError),
    #[error("Custom error: {0}")]
    CustomError(String),
}

impl YuError {
    pub fn new(message: &str) -> Self {
        YuError::CustomError(message.to_string())
    }
}
