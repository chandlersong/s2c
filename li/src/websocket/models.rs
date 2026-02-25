use crate::errors::LiError;
use actix::Message as ActixMessage;
use log::warn;

pub trait WebSocketMessage: ActixMessage<Result = ()> + Send + Sync + Clone + 'static {
    fn from_text(_: &str) -> Result<Self, LiError> {
        warn!("binary messages are not supported by default. Please implement from_binary for your message type.");
        Err(LiError::CustomError(
            "Text messages are not supported by default. Please implement from_text for your message type.".to_string(),
        ))
    }
    fn from_binary(_: Vec<u8>) -> Result<Self, LiError> {
        warn!("binary messages are not supported by default. Please implement from_binary for your message type.");
        Err(LiError::CustomError(
            "Binary messages are not supported by default. Please implement from_binary for your message type.".to_string(),
        ))
    }
}
