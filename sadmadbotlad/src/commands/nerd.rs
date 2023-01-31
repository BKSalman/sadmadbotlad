use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::irc::to_irc_message;

use super::Command;

pub struct NerdCommand {
    message: String,
}

impl NerdCommand {
    pub fn new(message: String) -> Self {
        Self { message }
    }
}

#[async_trait]
impl Command for NerdCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        ws_sender
            .send(Message::Text(to_irc_message(&self.message)))
            .await?;
        Ok(())
    }
}
