use log::{error, trace};
use teloxide::prelude::{ChatId, Requester};
use teloxide::Bot;

#[derive(Clone)]
struct OpsBot {
    bot: Bot,
    chat_id: ChatId,
}

impl OpsBot {
    fn new(client_id: &str, chat_id: i64) -> Self {
        let bot = Bot::new(client_id);
        let chat_id = ChatId(chat_id);
        Self { bot, chat_id }
    }

    async fn send(&self, text: &str) {
        match self.bot.send_message(self.chat_id, text).await{
            Ok(_) => trace!("消息已发送！"),
            Err(e) => println!("发送失败: {:?}", e),
        }
    }
}
