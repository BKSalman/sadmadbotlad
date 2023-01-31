use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::{irc::to_irc_message, APP};

use super::Command;

pub struct PingCommand {
    command: String,
}

impl PingCommand {
    pub fn new(command: String) -> Self {
        Self { command }
    }
}

#[async_trait]
impl Command for PingCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        if self.command.is_ascii() {
            ws_sender
                .send(Message::Text(to_irc_message(&format!(
                    "{}pong",
                    APP.config.cmd_delim
                ))))
                .await?;
        } else {
            ws_sender
                .send(Message::Text(to_irc_message(&format!(
                    "{}يكز",
                    APP.config.cmd_delim
                ))))
                .await?;
        }
        Ok(())
    }
}
