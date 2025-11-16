use futures_util::SinkExt;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use crate::{APP, song_requests::QueueMessages};

pub async fn sr_ws_server(
    queue_sender: mpsc::UnboundedSender<QueueMessages>,
) -> anyhow::Result<()> {
    tracing::info!("Starting Sr WebSocket Server on port {}", APP.config.port);

    let ip_address = Ipv4Addr::new(127, 0, 0, 1);
    let address = SocketAddrV4::new(ip_address, APP.config.port);
    let listener = TcpListener::bind(address).await?;

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let peer = stream.peer_addr()?;

                tracing::debug!("Songs Peer address: {}", peer);

                let queue_sender = queue_sender.clone();

                tokio::spawn(async move {
                    if let Err(e) = handle_connection(queue_sender, peer, stream).await {
                        tracing::error!("Error processing connection: {}", e)
                    }
                });
            }
            Err(e) => return Err(anyhow::anyhow!("Failed to accept client: {e}")),
        }
    }
}

async fn handle_connection(
    queue_sender: mpsc::UnboundedSender<QueueMessages>,
    peer: SocketAddr,
    stream: TcpStream,
) -> anyhow::Result<()> {
    let mut ws_stream = accept_async(stream).await.expect("Failed to accept");

    let (send, recv) = oneshot::channel();

    queue_sender
        .send(QueueMessages::GetQueue(send))
        .expect("request songs");

    let Ok(queue) = recv.await else {
        return Err(anyhow::anyhow!("Could not get queue"));
    };

    tracing::info!("Sending Queue to Peer {peer}");

    let Ok(queue) = serde_json::to_string(&queue) else {
        panic!("Could not parse queue to string");
    };

    if let Err(e) = ws_stream.send(Message::Text(queue.into())).await {
        tracing::error!("WebSocket server:: {e}");
        return Ok(());
    }

    Ok(())
}
