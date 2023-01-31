use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use libmpv::Mpv;
use tokio::{net::TcpStream, sync::RwLock};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::{irc::to_irc_message, song_requests::SrQueue};

use super::Command;

pub struct SkipSrCommand {
    queue: Arc<RwLock<SrQueue>>,
    mpv: Arc<Mpv>,
}

impl SkipSrCommand {
    pub fn new(queue: Arc<RwLock<SrQueue>>, mpv: Arc<Mpv>) -> Self {
        Self { queue, mpv }
    }
}

#[async_trait]
impl Command for SkipSrCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        let mut message = String::new();
        if let Some(song) = &self.queue.read().await.current_song {
            if self.mpv.playlist_next_force().is_ok() {
                message = to_irc_message(&format!("Skipped: {}", song.title));
            }
        } else {
            message = to_irc_message("No song playing");
        }

        ws_sender.send(Message::Text(message)).await?;
        Ok(())
    }
}
