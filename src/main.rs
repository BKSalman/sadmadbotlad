use std::{error::Error, sync::Arc};

use eyre::WrapErr;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use reqwest::Url;
use serde::Deserialize;
use serde_json::json;
use tokio::{net::TcpStream, sync::watch, task::JoinHandle};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use twitch::{EventSub, LiveStatus, EventSubResponse};

mod discord;
mod twitch;
mod util;

use discord::send_notification;
use util::install_eyre;

use crate::twitch::{WsEventSub, online_event, offline_event};

#[derive(Deserialize)]
struct ApiInfo {
    client_id: twitch_oauth2::ClientId,
    pub twitch_oauth: String,
    pub client_secret: twitch_oauth2::ClientSecret,
    pub secret: String,
    pub broadcaster_login: twitch_api::types::UserName,
}

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    // send_notification().await?;
    install_eyre()?;

    run().await.with_context(|| "when running application")?;

    Ok(())
}

async fn run() -> Result<(), eyre::Report> {
    match envy::from_env::<ApiInfo>() {
        Ok(api_info) => {
            let (socket, _response) = connect_async(
                Url::parse("wss://eventsub-beta.wss.twitch.tv/ws").expect("Url parsed"),
            )
            .await?;

            let _live = LiveStatus::Offline {
                url: String::from("https://twitch.tv/sadmadladsalman"),
            };
            
            delete_subs(api_info.client_id, api_info.twitch_oauth).await?;
            
            // let (sender, recv) = watch::channel(live);

            handle_socket(socket).await;
        }
        Err(e) => {
            panic!("Api Info {e}");
        }
    }

    Ok(())
}

async fn flatten<T>(handle: JoinHandle<Result<T, eyre::Report>>) -> Result<T, eyre::Report> {
    match handle.await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(err),
        Err(e) => Err(e).wrap_err_with(|| "handling failed"),
    }
}

async fn handle_socket(
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    // watch: watch::Receiver<LiveStatus>,
) {
    let (sender, receiver) = socket.split();

    if let Err(e) = tokio::try_join!(
        // flatten(tokio::spawn(write(sender, watch))),
        flatten(tokio::spawn(read(receiver, sender)))
    )
    .wrap_err_with(|| "in stream join")
    {
        eprintln!("socket failed {e}")
    }
}

async fn read(
    mut recv: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    mut sender: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
) -> Result<(), eyre::Report> {
    while let Some(msg) = recv.next().await {
        match msg {
            Ok(Message::Ping(_)) => {
                // println!("ping");
                sender.send(Message::Pong(vec![])).await?;
            }
            Ok(msg) => {
                let msg = msg.to_string();
                if msg.contains("connection unused") {
                    println!("{msg}");
                    return Err(eyre::Report::msg("connection unsed... disconnecting"));
                }
                let json_msg = match serde_json::from_str::<WsEventSub>(&msg) {
                    Ok(json_msg) => json_msg,
                    Err(e) => {
                        panic!("eventsub json Error: {} \n\n Message: {}", e, msg);
                    }
                };
                
                if let Some(session_id) = json_msg.payload.session {
                    let http_client = reqwest::Client::new();

                    let response = http_client
                        .post("https://api.twitch.tv/helix/eventsub/subscriptions")
                        .header(
                            "Authorization",
                            format!(
                                "Bearer {}",
                                std::env::var("TWITCH_OAUTH").expect("twitch oauth token")
                            ),
                        )
                        .header("Client-Id", std::env::var("CLIENT_ID").expect("twitch client id"))
                        .json(&online_event(session_id.id.clone()))
                        .send()
                        .await?
                        .text()
                        .await?;

                    println!("{:?}", response);

                    let response = http_client
                        .post("https://api.twitch.tv/helix/eventsub/subscriptions")
                        .header(
                            "Authorization",
                            format!(
                                "Bearer {}",
                                std::env::var("TWITCH_OAUTH").expect("twitch oauth token")
                            ),
                        )
                        .header("Client-Id", std::env::var("CLIENT_ID").expect("twitch client id"))
                        .json(&offline_event(session_id.id))
                        .send()
                        .await?
                        .text()
                        .await?;
                    // let response = serde_json::from_str(&response);

                    println!("{:?}", response);
                } else if let Some(subscription) = json_msg.payload.subscription {
                    println!("{subscription:?}");
                    println!("change is_live state or just send in discord");

                    if subscription.r#type == "stream.online" {
                        send_notification().await?;
                    }
                }
            }
            Err(e) => println!("{e}"),
        }
    }
    Ok(())
}

// Sends live status to clients.
async fn write(
    mut sender: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    mut watch: watch::Receiver<LiveStatus>,
) -> Result<(), eyre::Report> {
    while watch.changed().await.is_ok() {
        let val = watch.borrow().clone();

        let Ok(msg) = val.to_message() else {
            continue;
        };

        if let Err(error) = sender.send(msg).await {
            if let Some(e) = error.source() {
                let Some(tokio_tungstenite::tungstenite::error::Error::ConnectionClosed) =
                    e.downcast_ref()
                else {
                    return Err(error.into());
                };
            }
        }
    }
    Ok(())
}

async fn delete_subs(client_id: twitch_oauth2::ClientId, twitch_oauth: String) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();
    
    let subs = http_client.get("https://api.twitch.tv/helix/eventsub/subscriptions")
        .bearer_auth(twitch_oauth.clone())
        .header("Client-Id", client_id.as_str())
        .send().await?
        .json::<EventSubResponse>()
        .await?;
    
    for sub in subs.data {
        
        println!("deleting sub with id {}", sub.id);
        
        http_client.delete("https://api.twitch.tv/helix/eventsub/subscriptions")
        .bearer_auth(twitch_oauth.clone())
        .header("Client-Id", client_id.as_str())
        .json(&json!({
                "id": sub.id
            }))
        .send().await?;

    }
    
    Ok(())
}

