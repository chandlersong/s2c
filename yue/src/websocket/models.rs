use crate::errors::YueError;
use actix::Message as ActixMessage;

pub trait WebSocketTextMessage: ActixMessage<Result = ()> + Send + Sync + Clone + 'static {
    fn from_text(text: &str) -> Result<Self, YueError>;
}
