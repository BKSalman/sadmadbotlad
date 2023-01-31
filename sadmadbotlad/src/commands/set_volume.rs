use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use libmpv::Mpv;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::irc::to_irc_message;

use super::Command;

pub struct SetVolumeCommand {
    mpv: Arc<Mpv>,
    volume: i64,
}

impl SetVolumeCommand {
    pub fn new(mpv: Arc<Mpv>, volume: i64) -> Self {
        Self { mpv, volume }
    }
}

#[async_trait]
impl Command for SetVolumeCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        if let Err(e) = self.mpv.set_property("volume", self.volume) {
            println!("{e}");
        }

        ws_sender
            .send(Message::Text(to_irc_message(&format!(
                "Volume set to {}",
                self.volume
            ))))
            .await?;
        Ok(())
    }
}
