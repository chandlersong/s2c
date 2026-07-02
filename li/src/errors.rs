use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::create_table::CreateTableError;
use aws_sdk_dynamodb::operation::update_time_to_live::UpdateTimeToLiveError;
use log::SetLoggerError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LiError {
    #[error("DynamoDB error: {0}")]
    DynamoDBError(#[from] aws_sdk_dynamodb::Error),
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

impl From<SdkError<CreateTableError>> for LiError {
    fn from(err: SdkError<CreateTableError>) -> Self {
        LiError::DynamoDBError(aws_sdk_dynamodb::Error::from(err))
    }
}

impl From<SdkError<UpdateTimeToLiveError>> for LiError {
    fn from(err: SdkError<UpdateTimeToLiveError>) -> Self {
        LiError::DynamoDBError(aws_sdk_dynamodb::Error::from(err))
    }
}
