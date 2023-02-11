use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use libmpv::Mpv;
use tokio::{net::TcpStream, sync::RwLock};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::{irc::to_irc_message, song_requests::Queue};

use super::Command;

pub struct PlayCommand {
    queue: Arc<RwLock<Queue>>,
    mpv: Arc<Mpv>,
}

impl PlayCommand {
    pub fn new(queue: Arc<RwLock<Queue>>, mpv: Arc<Mpv>) -> Self {
        Self { queue, mpv }
    }
}

#[async_trait]
impl Command for PlayCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        if let Some(song) = &self.queue.read().await.current_song {
            if let Err(e) = self.mpv.unpause() {
                println!("{e}");
            }

            ws_sender
                .send(Message::Text(to_irc_message(&format!(
                    "Resumed {}",
                    song.title
                ))))
                .await?;
        } else {
            ws_sender
                .send(Message::Text(to_irc_message("Queue is empty")))
                .await?;
        }
        Ok(())
    }
}
