use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::irc::to_irc_message;

use super::Command;

pub struct RulesCommand;

const RULES: &str = include_str!("../../rules.txt");

#[async_trait]
impl Command for RulesCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        for rule in RULES.lines() {
            ws_sender.send(Message::Text(to_irc_message(rule))).await?;
        }
        Ok(())
    }
}
