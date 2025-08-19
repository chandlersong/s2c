use aws_smithy_types::error::operation::BuildError;
use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub struct MaesterError {
    message: String,
}

impl fmt::Display for MaesterError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Custom Error: {}", self.message)
    }
}

impl MaesterError {
    pub fn new(message: &str) -> MaesterError {
        MaesterError {
            message: message.to_string(),
        }
    }
}

// 实现 Error trait，用于提供错误信息
impl Error for MaesterError {}

impl From<aws_sdk_dynamodb::Error> for MaesterError {
    fn from(error: aws_sdk_dynamodb::Error) -> Self {
        MaesterError {
            message: format!("aws dynamodb Error: {}", error),
        }
    }
}

impl From<BuildError> for MaesterError {
    fn from(error: BuildError) -> Self {
        MaesterError {
            message: format!("aws build Error: {}", error),
        }
    }
}
