use futures_util::SinkExt;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{
    accept_async,
    tungstenite::{Message, Result},
};

use crate::{SrEvent, APP};

pub async fn sr_ws_server() -> Result<(), eyre::Report> {
    println!("Starting Sr WebSocket Server");

    let ip_address = Ipv4Addr::new(127, 0, 0, 1);
    let address = SocketAddrV4::new(ip_address, APP.config.port);
    let listener = TcpListener::bind(address).await?;

    while let Ok((stream, _)) = listener.accept().await {
        let peer = stream.peer_addr()?;

        // println!("Songs Peer address: {}", peer);

        tokio::spawn(accept_connection(peer, stream));
    }

    Ok(())
}

async fn accept_connection(peer: SocketAddr, stream: TcpStream) {
    if let Err(e) = handle_connection(peer, stream).await {
        match e {
            tokio_tungstenite::tungstenite::Error::ConnectionClosed
            | tokio_tungstenite::tungstenite::Error::Protocol(_)
            | tokio_tungstenite::tungstenite::Error::Utf8 => (),
            err => println!("Error processing connection: {}", err),
        }
    }
}

async fn handle_connection(peer: SocketAddr, stream: TcpStream) -> Result<()> {
    let sr_sender = APP.sr_sender.clone();

    let mut sr_receiver = sr_sender.subscribe();

    let mut ws_stream = accept_async(stream).await.expect("Failed to accept");

    sr_sender
        .send(SrEvent::QueueRequest)
        .expect("request songs");

    while let Ok(msg) = sr_receiver.recv().await {
        println!("Sending Queue to Peer {peer}");
        match msg {
            SrEvent::QueueRequest => {}
            SrEvent::QueueResponse(queue) => {
                let Ok(queue) = serde_json::to_string(&*queue.read().await) else {
                    panic!("Could not parse queue to string");
                };
                match ws_stream.send(Message::Text(queue)).await {
                    Ok(_) => {
                        break;
                    }
                    Err(e) => {
                        println!("WebSocket server:: {e}");
                        break;
                    }
                }
            }
        }
    }

    println!("sent");

    Ok(())
}
