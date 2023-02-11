use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::{net::TcpStream, sync::mpsc};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::{
    irc::to_irc_message,
    twitch::{get_title, TwitchTokenMessages},
};

use super::Command;

pub struct GetTitleCommand {
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
}

impl GetTitleCommand {
    pub fn new(token_sender: mpsc::UnboundedSender<TwitchTokenMessages>) -> Self {
        Self { token_sender }
    }
}

#[async_trait]
impl Command for GetTitleCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        let title = get_title(self.token_sender.to_owned()).await?;
        ws_sender
            .send(Message::Text(to_irc_message(&format!(
                "Current stream title: {}",
                title
            ))))
            .await?;
        Ok(())
    }
}
