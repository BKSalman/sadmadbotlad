pub mod alert_test;
pub mod commercial_break;
pub mod current_song;
pub mod current_song_spotify;
pub mod database;
pub mod discord;
pub mod get_title;
pub mod get_volume;
pub mod invalid;
pub mod mods_only;
pub mod nerd;
pub mod ping;
pub mod pixel_perfect;
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
pub mod vote_skip;
pub mod warranty;
pub mod working_on;

pub use alert_test::AlertTestCommand;
pub use commercial_break::CommercialBreakCommand;
pub use current_song::CurrentSongCommand;
pub use current_song_spotify::CurrentSongSpotifyCommand;
pub use database::DatabaseCommand;
pub use discord::DiscordCommand;
pub use get_title::GetTitleCommand;
pub use get_volume::GetVolumeCommand;
pub use invalid::InvalidCommand;
pub use mods_only::ModsOnlyCommand;
pub use nerd::NerdCommand;
pub use ping::PingCommand;
pub use pixel_perfect::PixelPerfectCommand;
pub use play::PlayCommand;
pub use play_spotify::PlaySpotifyCommand;
pub use queue::QueueCommand;
pub use rules::RulesCommand;
pub use rust_warranty::RustWarrantyCommand;
pub use set_title::SetTitleCommand;
pub use set_volume::SetVolumeCommand;
pub use seventv::SevenTvCommand;
pub use skip_sr::SkipSrCommand;
pub use sr::SrCommand;
pub use stop::StopCommand;
pub use stop_spotify::StopSpotifyCommand;
pub use vote_skip::VoteSkipCommand;
pub use warranty::WarrantyCommand;
pub use working_on::WorkingOnCommand;

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
