use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::irc::to_irc_message;

use super::Command;

pub struct WarrantyCommand;

const WARRANTY: &str = include_str!("../../warranty.txt");

#[async_trait]
impl Command for WarrantyCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        ws_sender
            .send(Message::Text(to_irc_message(WARRANTY)))
            .await?;
        Ok(())
    }
}
