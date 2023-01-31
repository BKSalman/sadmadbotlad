pub mod alert_test;
pub mod commercial_break;
pub mod current_song;
pub mod current_song_spotify;
pub mod get_title;
pub mod get_volume;
pub mod invalid;
pub mod mods_only;
pub mod ping;
pub mod play;
pub mod play_spotify;
pub mod queue;
pub mod rules;
pub mod rust_warranty;
pub mod set_title;
pub mod set_volume;
pub mod seventv;
pub mod skip_sr;
pub mod sr;
pub mod stop;
pub mod stop_spotify;
pub mod warranty;

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
