use log::{error, trace};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::{broadcast, RwLock};

pub struct SubscriberItem<T: 'static + Clone + Debug> {
    pub is_new: bool,
    pub rx: Receiver<T>,
}


///
///  单个
///
///
#[derive(Debug, Clone)]
pub struct Bus<T: 'static + Clone + Debug> {
    topics: Arc<RwLock<HashMap<String, broadcast::Sender<T>>>>,
}

impl<T: Clone + Debug> Bus<T> {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            topics: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn exists(&self, topic: &str) -> bool {
        self.topics.read().await.contains_key(topic)
    }

    async fn create_or_get_topic(&self, topic_name: &str) -> (Sender<T>, bool) {
        if let Some(value) = self.topics.read().await.get(topic_name) {
            return (value.clone(), false);
        }
        trace!("Topic {} not exist, auto create", topic_name);
        let (tx, _) = broadcast::channel::<T>(1000);
        self.topics.write().await.insert(topic_name.into(), tx.clone());
        (tx.clone(), true)
    }

    pub async fn subscribe(&self, topic: &str) -> SubscriberItem<T> {
        let (tx, is_new) = self.create_or_get_topic(topic).await;
        SubscriberItem {
            is_new,
            rx: tx.subscribe(),
        }
    }


    pub async fn publish(&self, topic: &str, message: T) {
        if let Some(sender) = self.topics.read().await.get(topic) {
            match sender.send(message) {
                Ok(_) => {}
                Err(e) => {
                    error!("Topic {} not fail,error is {}", topic,e);
                }
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::tools::bus::Bus;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_simple_topic() {
        let bus = Bus::<u32>::new();
        let bus_sender = bus.clone();
        tokio::spawn(async move {
            bus_sender.publish("bbb", 2).await;
            bus_sender.publish("aaa", 1).await;
        });

        let mut a_rx = bus.subscribe("aaa").await.rx;
        let mut b_rx = bus.subscribe("bbb").await.rx;
        let actual_a = a_rx.recv().await.unwrap();
        let actual_b = b_rx.recv().await.unwrap();
        assert_eq!(actual_a, 1);
        assert_eq!(actual_b, 2);
    }


    ///
    /// 测试一些没有subscribe的情况
    #[tokio::test]
    async fn test_no_subscriber() {
        let bus = Bus::<u32>::new();
        let bus_sender = bus.clone();
        bus_sender.publish("aaa", 1).await;
    }

    #[tokio::test]
    async fn test_no_publisher() {
        let bus = Bus::<u32>::new();
        let subscribe_item = bus.subscribe("aaa").await;

        assert!(subscribe_item.is_new);

        let mut rx = subscribe_item.rx;
        let timeout_duration = Duration::from_millis(200);
        let mut not_receive = false;
        match timeout(timeout_duration, rx.recv()).await {
            Ok(_) => {}
            Err(_) => {
                not_receive = true;
            }
        }
        assert!(not_receive);
    }
}
