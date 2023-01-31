use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::irc::to_irc_message;

use super::Command;

pub struct VoteSkipCommand {
    voters: usize,
}

impl VoteSkipCommand {
    pub fn new(voters: usize) -> Self {
        Self { voters }
    }
}

#[async_trait]
impl Command for VoteSkipCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        let message = format!("{}/5 skip votes", self.voters);

        ws_sender
            .send(Message::Text(to_irc_message(&message)))
            .await?;
        Ok(())
    }
}
