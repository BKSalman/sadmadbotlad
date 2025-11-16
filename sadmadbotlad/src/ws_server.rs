use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    time::Duration,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite::Message};

use crate::{APP, Alert, db::DBMessage};

pub async fn ws_server(
    alerts_sender: tokio::sync::broadcast::Sender<Alert>,
    db_tx: std::sync::mpsc::Sender<DBMessage>,
) -> anyhow::Result<()> {
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
        let db_tx = db_tx.clone();
        let alerts_sender = alerts_sender.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(alerts_sender, peer, stream, db_tx).await {
                tracing::error!("Error processing connection: {:?}", e);
            }
        });
    }

    Ok(())
}

async fn handle_connection(
    alerts_sender: tokio::sync::broadcast::Sender<Alert>,
    peer: SocketAddr,
    stream: TcpStream,
    db_tx: std::sync::mpsc::Sender<DBMessage>,
) -> anyhow::Result<()> {
    let alerts_receiver = alerts_sender.subscribe();

    let ws_stream = accept_async(stream).await.expect("Failed to accept");

    let (ws_sender, mut ws_receiver) = ws_stream.split();

    let (ws_sender_tx, ws_sender_rx) = tokio::sync::mpsc::channel(5);

    let t_handle = tokio::spawn(async move {
        handle_websocket_send(alerts_receiver, ws_sender, ws_sender_rx, peer).await;
    });

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
                    let (tx, rx) = crate::oneshot();
                    db_tx.send(DBMessage::GetEvents(tx)).unwrap();
                    let Ok(events) = rx.recv() else {
                        continue;
                    };

                    let events = serde_json::to_string(
                        &events
                            .into_iter()
                            .rev()
                            .map(|a| Alert {
                                new: false,
                                r#type: a.alert_type,
                            })
                            .collect::<Vec<_>>(),
                    )?;

                    tracing::debug!("events: {events}");

                    ws_sender_tx
                        .send(Message::Text(format!("db::{}", events).into()))
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
    tracing::debug!("aborted websocket thread");

    Ok(())
}

async fn handle_websocket_send(
    mut front_end_event_receiver: tokio::sync::broadcast::Receiver<Alert>,
    mut ws_sender: SplitSink<WebSocketStream<TcpStream>, Message>,
    mut ws_sender_rx: tokio::sync::mpsc::Receiver<Message>,
    _peer: SocketAddr,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(5));

    loop {
        tokio::select! {
            Ok(msg) = front_end_event_receiver.recv() => {
                tracing::debug!("Sending Ws:: {msg:?}");

                let alert = serde_json::to_string(&msg).expect("alert");

                if let Err(e) = ws_sender.send(Message::Text(alert.into())).await {
                    tracing::error!("websocket sender: {e}");
                }
            }
            Some(send) = ws_sender_rx.recv() => {
                if let Err(e) = ws_sender.send(send).await {
                    tracing::error!("websocket sender: {e}");
                }
            }
            _ = interval.tick() => {
                if let Err(e) = ws_sender.send(Message::Ping(tokio_tungstenite::tungstenite::Bytes::new())).await {
                    tracing::error!("websocket sender: {e}");
                }
            }
        }
    }
}
