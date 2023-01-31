use std::process;

use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::irc::to_irc_message;

use super::Command;

pub struct PlaySpotifyCommand;

#[async_trait]
impl Command for PlaySpotifyCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        if process::Command::new("./scripts/play_spotify.sh")
            .spawn()
            .is_ok()
        {
            ws_sender
                .send(Message::Text(to_irc_message("Started playing spotify")))
                .await?;
        } else {
            println!("no script");
        }
        Ok(())
    }
}
