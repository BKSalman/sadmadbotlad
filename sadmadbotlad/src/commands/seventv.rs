use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::irc::to_irc_message;

use super::Command;

pub struct SevenTvCommand {
    query: String,
}

impl SevenTvCommand {
    pub fn new(query: String) -> Self {
        Self { query }
    }
}

#[async_trait]
impl Command for SevenTvCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        let query = urlencoding::encode(&self.query);

        ws_sender
            .send(Message::Text(to_irc_message(format!(
                "https://7tv.app/emotes?query={query}"
            ))))
            .await?;
        Ok(())
    }
}
