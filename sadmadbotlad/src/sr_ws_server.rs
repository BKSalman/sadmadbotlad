use futures_util::SinkExt;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
};
use tokio_tungstenite::{
    accept_async,
    tungstenite::{Message, Result},
};

use crate::{song_requests::QueueMessages, APP};

pub async fn sr_ws_server(
    queue_sender: mpsc::UnboundedSender<QueueMessages>,
) -> Result<(), eyre::Report> {
    println!("Starting Sr WebSocket Server");

    let ip_address = Ipv4Addr::new(127, 0, 0, 1);
    let address = SocketAddrV4::new(ip_address, APP.config.port);
    let listener = TcpListener::bind(address).await?;

    while let Ok((stream, _)) = listener.accept().await {
        let peer = stream.peer_addr()?;

        // println!("Songs Peer address: {}", peer);

        tokio::spawn(accept_connection(queue_sender.to_owned(), peer, stream));
    }

    Ok(())
}

async fn accept_connection(
    queue_sender: mpsc::UnboundedSender<QueueMessages>,
    peer: SocketAddr,
    stream: TcpStream,
) {
    if let Err(e) = handle_connection(queue_sender, peer, stream).await {
        println!("Error processing connection: {}", e)
    }
}

async fn handle_connection(
    queue_sender: mpsc::UnboundedSender<QueueMessages>,
    peer: SocketAddr,
    stream: TcpStream,
) -> eyre::Result<()> {
    let mut ws_stream = accept_async(stream).await.expect("Failed to accept");

    let (send, recv) = oneshot::channel();

    queue_sender
        .send(QueueMessages::GetQueue(send))
        .expect("request songs");

    let Ok(queue) = recv.await else {
        return Err(eyre::eyre!("Could not get queue"));
    };

    println!("Sending Queue to Peer {peer}");

    let Ok(queue) = serde_json::to_string(&queue) else {
        panic!("Could not parse queue to string");
    };

    if let Err(e) = ws_stream.send(Message::Text(queue)).await {
        println!("WebSocket server:: {e}");
        return Ok(());
    }

    Ok(())
}
