use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::{net::TcpStream, sync::RwLock};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::{irc::to_irc_message, song_requests::Queue};

use super::Command;

pub struct QueueCommand {
    queue: Arc<RwLock<Queue>>,
}

impl QueueCommand {
    pub fn new(queue: Arc<RwLock<Queue>>) -> Self {
        QueueCommand { queue }
    }
}

#[async_trait]
impl Command for QueueCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        println!("{:#?}", self.queue.read().await.queue);
        ws_sender
            .send(Message::Text(to_irc_message(
                "Check the queue at: https://f5rfm.bksalman.com",
            )))
            .await?;
        Ok(())
    }
}
