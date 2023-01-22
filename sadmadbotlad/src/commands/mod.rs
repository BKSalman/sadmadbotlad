pub mod current_song;
pub mod ping;
pub mod queue;
pub mod seventv;
pub mod skip_sr;
pub mod sr;

use async_trait::async_trait;
use futures_util::stream::SplitSink;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

#[async_trait]
pub trait Command {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()>;
}

pub async fn execute(
    command: impl Command,
    ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
) -> eyre::Result<()> {
    command.execute(ws_sender).await?;

    Ok(())
}
