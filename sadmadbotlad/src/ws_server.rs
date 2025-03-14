use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
    time::Duration,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{
    accept_async,
    tungstenite::{Message, Result},
    WebSocketStream,
};

use crate::{db::Store, Alert, APP};

pub async fn ws_server(
    alerts_sender: tokio::sync::broadcast::Sender<Alert>,
    store: Arc<Store>,
) -> Result<(), eyre::Report> {
    let port = APP.config.port + 1000;
    tracing::info!("Starting WebSocket Server on port {port}");

    let ip_address = Ipv4Addr::new(127, 0, 0, 1);
    let address = SocketAddrV4::new(ip_address, port);
    let listener = TcpListener::bind(address).await?;

    while let Ok((stream, _)) = listener.accept().await {
        let peer = stream
            .peer_addr()
            .expect("connected streams should have a peer address");

        tracing::debug!("Peer address: {}", peer);

        tokio::spawn(accept_connection(
            alerts_sender.to_owned(),
            peer,
            stream,
            store.clone(),
        ));
    }

    Ok(())
}

async fn accept_connection(
    alerts_sender: tokio::sync::broadcast::Sender<Alert>,
    peer: SocketAddr,
    stream: TcpStream,
    store: Arc<Store>,
) {
    if let Err(e) = handle_connection(alerts_sender, peer, stream, store).await {
        tracing::error!("Error processing connection: {}", e)
    }
}

async fn handle_connection(
    alerts_sender: tokio::sync::broadcast::Sender<Alert>,
    peer: SocketAddr,
    stream: TcpStream,
    store: Arc<Store>,
) -> eyre::Result<()> {
    let alerts_receiver = alerts_sender.subscribe();

    let ws_stream = accept_async(stream).await.expect("Failed to accept");

    let (ws_sender, mut ws_receiver) = ws_stream.split();

    let ws_sender = Arc::new(tokio::sync::Mutex::new(ws_sender));

    let t_handle = tokio::spawn(handle_front_end_events(
        alerts_receiver,
        ws_sender.clone(),
        peer,
    ));

    let forever = {
        let ws_sender = ws_sender.clone();

        tokio::task::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));

            loop {
                interval.tick().await;
                ws_sender
                    .lock()
                    .await
                    .send(Message::Ping(vec![]))
                    .await
                    .expect("send ping");
            }
        })
    };

    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(Message::Close(msg)) => {
                tracing::debug!("close message: {msg:?}");
                // ws_sender.lock().await.close().await?;
                break;
            }
            Ok(Message::Text(msg)) => {
                if msg.starts_with("db") {
                    tracing::debug!("db was requested ");
                    let events: Vec<_> = store
                        .get_events()
                        .await?
                        .into_iter()
                        .map(|e| Alert {
                            new: false,
                            r#type: e.alert_type,
                        })
                        .rev()
                        .collect();

                    let events = serde_json::to_string(&events)?;

                    ws_sender
                        .lock()
                        .await
                        .send(Message::Text(format!("db::{}", events)))
                        .await?;

                    continue;
                }

                tracing::debug!("text message: {msg} - from client {peer}");
                let alert = serde_json::from_str::<Alert>(&msg).expect("alert");
                alerts_sender.send(alert).expect("send alert");
            }
            Ok(Message::Pong(_ping)) => {
                // tracing::debug!("Events Ws:: Pong {ping:?} - from client {peer}");
            }
            Ok(msg) => {
                tracing::debug!("message: {msg:?} - from client {peer}");
                // ws_sender.lock().await.send(msg).await?;
            }
            Err(e) => {
                tracing::error!("client error: {e}");
                break;
            }
        }
    }

    t_handle.abort();
    forever.abort();
    tracing::debug!("aborted websocket thread");

    Ok(())
}

async fn handle_front_end_events(
    mut front_end_event_receiver: tokio::sync::broadcast::Receiver<Alert>,
    ws_sender: Arc<tokio::sync::Mutex<SplitSink<WebSocketStream<TcpStream>, Message>>>,
    _peer: SocketAddr,
) {
    while let Ok(msg) = front_end_event_receiver.recv().await {
        tracing::debug!("Sending Ws:: {msg:?}");

        let alert = serde_json::to_string(&msg).expect("alert");

        if let Err(e) = ws_sender.lock().await.send(Message::Text(alert)).await {
            tracing::error!("{e}");
            break;
        }
    }
}
