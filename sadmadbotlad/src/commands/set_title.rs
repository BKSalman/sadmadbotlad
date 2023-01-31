use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::{irc::to_irc_message, twitch::set_title, ApiInfo};

use super::Command;

pub struct SetTitleCommand {
    title: String,
    api_info: Arc<ApiInfo>,
}

impl SetTitleCommand {
    pub fn new(api_info: Arc<ApiInfo>, title: String) -> Self {
        Self { api_info, title }
    }
}

#[async_trait]
impl Command for SetTitleCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        set_title(&self.title, &self.api_info).await?;
        ws_sender
            .send(Message::Text(to_irc_message(&format!(
                "Set stream title to: {}",
                self.title
            ))))
            .await?;
        Ok(())
    }
}
