use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::{net::TcpStream, sync::RwLock};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::{irc::to_irc_message, song_requests::Queue};

use super::Command;

pub struct CurrentSongCommand {
    queue: Arc<RwLock<Queue>>,
}

impl CurrentSongCommand {
    pub fn new(queue: Arc<RwLock<Queue>>) -> Self {
        Self { queue }
    }
}

#[async_trait]
impl Command for CurrentSongCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        if let Some(current_song) = self.queue.read().await.current_song.as_ref() {
            ws_sender
                .send(Message::Text(to_irc_message(&format!(
                    "current song: {} , requested by {} - {}",
                    current_song.title, current_song.user, current_song.url,
                ))))
                .await?;
        } else {
            ws_sender
                .send(Message::Text(to_irc_message("No song playing")))
                .await?;
        }
        Ok(())
    }
}
