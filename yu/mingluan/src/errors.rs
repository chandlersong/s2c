use thiserror::Error;

#[derive(Error, Debug)]
pub enum MingLuanError {
    #[error("r2d2 error: {0}")]
    R2D2Error(#[from] r2d2::Error),
    #[error("Custom error: {0}")]
    CustomError(String),
}

impl MingLuanError {
    pub fn new(message: &str) -> Self {
        MingLuanError::CustomError(message.to_string())
    }
}
