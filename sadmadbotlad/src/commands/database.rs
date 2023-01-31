use std::sync::Arc;

use async_trait::async_trait;
use futures_util::stream::SplitSink;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::db::Store;

use super::Command;

pub struct DatabaseCommand {
    store: Arc<Store>,
}

impl DatabaseCommand {
    pub fn new(store: Arc<Store>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Command for DatabaseCommand {
    async fn execute(
        &self,
        _ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        let events = self.store.get_events().await?;

        println!("{events:#?}");
        Ok(())
    }
}
