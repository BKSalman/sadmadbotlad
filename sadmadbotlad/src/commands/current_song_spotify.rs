use std::process;

use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::irc::to_irc_message;

use super::Command;

pub struct CurrentSongSpotifyCommand;

#[async_trait]
impl Command for CurrentSongSpotifyCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        let cmd = process::Command::new("./scripts/current_spotify_song.sh").output()?;
        let output = String::from_utf8(cmd.stdout)?;

        ws_sender
            .send(Message::Text(to_irc_message(&format!(
                "Current Spotify song: {}",
                output
            ))))
            .await?;
        Ok(())
    }
}
