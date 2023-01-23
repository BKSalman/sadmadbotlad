use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::{
    net::TcpStream,
    sync::{mpsc::Sender, RwLock},
};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::{
    irc::to_irc_message,
    song_requests::{SongRequest, SrQueue},
    ApiInfo,
};

use super::Command;

pub struct SrCommand {
    queue: Arc<RwLock<SrQueue>>,
    song: String,
    sender: String,
    song_sender: Sender<SongRequest>,
    api_info: Arc<RwLock<ApiInfo>>,
}

impl SrCommand {
    pub fn new(
        queue: Arc<RwLock<SrQueue>>,
        song: String,
        sender: String,
        song_sender: Sender<SongRequest>,
        api_info: Arc<RwLock<ApiInfo>>,
    ) -> Self {
        SrCommand {
            queue,
            song,
            sender,
            song_sender,
            api_info,
        }
    }
}

#[async_trait]
impl Command for SrCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        match self
            .queue
            .write()
            .await
            .sr(
                &self.song,
                &self.sender,
                self.song_sender.clone(),
                self.api_info.clone(),
            )
            .await
        {
            Ok(message) => {
                ws_sender.send(Message::Text(message)).await?;
            }
            Err(e) => match e.to_string().as_str() {
                "Not A Valid Youtube URL" => {
                    ws_sender
                        .send(Message::Text(to_irc_message("Not a valid youtube URL")))
                        .await?;
                }
                _ => panic!("{e}"),
            },
        }
        Ok(())
    }
}
