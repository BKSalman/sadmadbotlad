use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{
    accept_async,
    tungstenite::{Message, Result},
    WebSocketStream,
};

use crate::FrontEndEvent;

pub async fn ws_server(
    front_end_event_sender: tokio::sync::broadcast::Sender<FrontEndEvent>,
) -> Result<(), eyre::Report> {
    println!("Starting WebSocket Server");

    let ip_address = Ipv4Addr::new(127, 0, 0, 1);
    let address = SocketAddrV4::new(ip_address, 3000);
    let listener = TcpListener::bind(address).await?;

    while let Ok((stream, _)) = listener.accept().await {
        let peer = stream
            .peer_addr()
            .expect("connected streams should have a peer address");
        println!("Peer address: {}", peer);

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
    front_end_event_sender: tokio::sync::broadcast::Sender<FrontEndEvent>,
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
    front_end_event_sender: tokio::sync::broadcast::Sender<FrontEndEvent>,
) -> Result<()> {
    let front_end_event_receiver = front_end_event_sender.subscribe();

    let ws_stream = accept_async(stream).await.expect("Failed to accept");

    let (ws_sender, mut ws_receiver) = ws_stream.split();

    let ws_sender = Arc::new(tokio::sync::Mutex::new(ws_sender));

    let ws_senderc = ws_sender.clone();
    // ws_sender.send(Message::Ping(vec![])).await?;
    let t_handle = tokio::spawn(handle_front_end_events(
        front_end_event_receiver,
        ws_senderc,
        peer,
    ));

    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(Message::Close(msg)) => {
                println!("{msg:?}");
                ws_sender.lock().await.close().await?;
                break;
            }
            Ok(msg) => {
                println!("message: {msg:?} - from client {peer}");
                ws_sender.lock().await.send(msg).await?;
            }
            Err(e) => {
                println!("client error: {e}");
            }
        }
    }

    t_handle.abort();
    println!("aborted");

    Ok(())
}

async fn handle_front_end_events(
    mut front_end_event_receiver: tokio::sync::broadcast::Receiver<FrontEndEvent>,
    ws_senderc: Arc<tokio::sync::Mutex<SplitSink<WebSocketStream<TcpStream>, Message>>>,
    peer: SocketAddr,
) {
    while let Ok(msg) = front_end_event_receiver.recv().await {
        println!("Sending Ws:: {msg:?}");
        match msg {
            FrontEndEvent::Follow { follower } => {
                println!("lmao {peer}");
                match ws_senderc
                    .lock()
                    .await
                    .send(Message::Text(format!(
                        "follow_event::{}::port:{}",
                        follower, peer
                    )))
                    .await
                {
                    Ok(_) => {}
                    Err(e) => {
                        println!("{e}");
                        break;
                    }
                }
            }
        }
    }
}
