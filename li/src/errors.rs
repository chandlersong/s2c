use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::create_table::CreateTableError;
use aws_sdk_dynamodb::operation::update_time_to_live::UpdateTimeToLiveError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MaesterError {
    #[error("DynamoDB error: {0}")]
    DynamoDBError(#[from] aws_sdk_dynamodb::Error),
    #[error("Serde error: {0}")]
    SerdeError(#[from] serde_json::Error),
    #[error("Custom error: {0}")]
    CustomError(String),
}

impl From<SdkError<CreateTableError>> for MaesterError {
    fn from(err: SdkError<CreateTableError>) -> Self {
        MaesterError::DynamoDBError(aws_sdk_dynamodb::Error::from(err))
    }
}

impl From<SdkError<UpdateTimeToLiveError>> for MaesterError {
    fn from(err: SdkError<UpdateTimeToLiveError>) -> Self {
        MaesterError::DynamoDBError(aws_sdk_dynamodb::Error::from(err))
    }
}
