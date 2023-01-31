use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::irc::to_irc_message;

use super::Command;

pub struct WorkingOnCommand;

#[async_trait]
impl Command for WorkingOnCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        ws_sender
                .send(Message::Text(to_irc_message("You can check what I'm working in here: https://gist.github.com/BKSalman/090658c8f67cc94bfb9d582d5be68ed4")))
                .await?;
        Ok(())
    }
}
