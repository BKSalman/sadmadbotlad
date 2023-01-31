use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use libmpv::Mpv;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::irc::to_irc_message;

use super::Command;

pub struct GetVolumeCommand {
    mpv: Arc<Mpv>,
}

impl GetVolumeCommand {
    pub fn new(mpv: Arc<Mpv>) -> Self {
        Self { mpv }
    }
}

#[async_trait]
impl Command for GetVolumeCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        let Ok(volume) = self.mpv.get_property::<i64>("volume") else {
                            println!("volume error");
                            ws_sender.send(Message::Text(to_irc_message("No volume"))).await?;
                            return Ok(());
                        };
        ws_sender
            .send(Message::Text(to_irc_message(&format!(
                "Volume: {}",
                volume
            ))))
            .await?;
        Ok(())
    }
}
