use std::process;

use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::irc::to_irc_message;

use super::Command;

pub struct StopSpotifyCommand;

#[async_trait]
impl Command for StopSpotifyCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        if let Err(e) = process::Command::new("./scripts/pause_spotify.sh").spawn() {
            println!("{e:?}");
            return Ok(());
        }

        ws_sender
            .send(Message::Text(to_irc_message("Stopped playing spotify")))
            .await?;
        Ok(())
    }
}
