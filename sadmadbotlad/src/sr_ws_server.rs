use futures_util::SinkExt;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{
    accept_async,
    tungstenite::{Message, Result},
};

use crate::SrFrontEndEvent;

pub async fn sr_ws_server(
    front_end_event_sender: tokio::sync::broadcast::Sender<SrFrontEndEvent>,
) -> Result<(), eyre::Report> {
    println!("Starting Sr WebSocket Server");

    let ip_address = Ipv4Addr::new(127, 0, 0, 1);
    let address = SocketAddrV4::new(ip_address, 3000);
    let listener = TcpListener::bind(address).await?;

    while let Ok((stream, _)) = listener.accept().await {
        let peer = stream
            .peer_addr()
            .expect("connected streams should have a peer address");

        // println!("Songs Peer address: {}", peer);

        tokio::spawn(accept_connection(
            peer,
            stream,
            front_end_event_sender.clone(),
        ));
    }

    Ok(())
}

async fn accept_connection(
    peer: SocketAddr,
    stream: TcpStream,
    front_end_event_sender: tokio::sync::broadcast::Sender<SrFrontEndEvent>,
) {
    if let Err(e) = handle_connection(peer, stream, front_end_event_sender).await {
        match e {
            tokio_tungstenite::tungstenite::Error::ConnectionClosed
            | tokio_tungstenite::tungstenite::Error::Protocol(_)
            | tokio_tungstenite::tungstenite::Error::Utf8 => (),
            err => println!("Error processing connection: {}", err),
        }
    }
}

async fn handle_connection(
    peer: SocketAddr,
    stream: TcpStream,
    front_end_sr_sender: tokio::sync::broadcast::Sender<SrFrontEndEvent>,
) -> Result<()> {
    let mut front_end_sr_receiver = front_end_sr_sender.subscribe();

    let mut ws_stream = accept_async(stream).await.expect("Failed to accept");

    front_end_sr_sender
        .send(SrFrontEndEvent::QueueRequest)
        .expect("request songs");

    while let Ok(msg) = front_end_sr_receiver.recv().await {
        println!("Sending Queue to Peer {peer}");
        match msg {
            SrFrontEndEvent::QueueRequest => {}
            SrFrontEndEvent::QueueResponse(queue) => {
                let Ok(queue) = serde_json::to_string(&*queue.read().await) else {
                    panic!("Could not parse queue to string");
                };
                println!("{queue}");
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
