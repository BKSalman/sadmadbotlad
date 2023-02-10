use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::{irc::to_irc_message, twitch::get_title, ApiInfo, TwitchApiInfo};

use super::Command;

pub struct GetTitleCommand<'a> {
    api_info: &'a TwitchApiInfo,
}

impl<'a> GetTitleCommand<'a> {
    pub fn new(api_info: &'a TwitchApiInfo) -> Self {
        Self { api_info }
    }
}

#[async_trait]
impl<'a> Command for GetTitleCommand<'a> {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        let title = get_title(&self.api_info).await?;
        ws_sender
            .send(Message::Text(to_irc_message(&format!(
                "Current stream title: {}",
                title
            ))))
            .await?;
        Ok(())
    }
}
