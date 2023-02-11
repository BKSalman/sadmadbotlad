use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::{net::TcpStream, sync::mpsc};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::{
    irc::to_irc_message,
    twitch::{set_title, TwitchTokenMessages},
};

use super::Command;

pub struct SetTitleCommand {
    title: String,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
}

impl SetTitleCommand {
    pub fn new(token_sender: mpsc::UnboundedSender<TwitchTokenMessages>, title: String) -> Self {
        Self {
            token_sender,
            title,
        }
    }
}

#[async_trait]
impl Command for SetTitleCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        set_title(&self.title, self.token_sender.to_owned()).await?;
        ws_sender
            .send(Message::Text(to_irc_message(&format!(
                "Set stream title to: {}",
                self.title
            ))))
            .await?;
        Ok(())
    }
}
