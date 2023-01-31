use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use libmpv::Mpv;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::irc::to_irc_message;

use super::Command;

pub struct StopCommand {
    mpv: Arc<Mpv>,
}

impl StopCommand {
    pub fn new(mpv: Arc<Mpv>) -> Self {
        Self { mpv }
    }
}

#[async_trait]
impl Command for StopCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        if let Err(e) = self.mpv.pause() {
            println!("{e}");
        }

        ws_sender
            .send(Message::Text(to_irc_message(
                "Stopped playing, !play to resume",
            )))
            .await?;
        Ok(())
    }
}
