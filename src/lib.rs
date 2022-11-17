use std::error::Error;

use eyre::WrapErr;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use serde::Deserialize;
use serde_json::json;
use tokio::{net::TcpStream, sync::watch, task::JoinHandle};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

pub mod discord;
mod twitch;

use crate::twitch::{offline_event, online_event, TwitchApiResponse, WsEventSub};
use discord::send_notification;
use twitch::{EventSubResponse, LiveStatus};

#[derive(Deserialize)]
pub struct ApiInfo {
    pub client_id: String,
    pub twitch_oauth: String,
    pub discord_token: String,
}

impl ApiInfo {
    pub fn new() -> Self {
        match envy::from_env::<ApiInfo>() {
            Ok(api_info) => api_info,
            Err(e) => {
                panic!("Api Info:: {e}");
            }
        }
    }

    pub async fn handle_socket(
        self,
        socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
        // watch: watch::Receiver<LiveStatus>,
    ) {
        let (sender, receiver) = socket.split();

        if let Err(e) = tokio::try_join!(
            // flatten(tokio::spawn(write(sender, watch))),
            flatten(tokio::spawn(read(self, receiver, sender)))
        )
        .wrap_err_with(|| "in stream join")
        {
            eprintln!("socket failed {e}")
        }
    }
}

async fn flatten<T>(handle: JoinHandle<Result<T, eyre::Report>>) -> Result<T, eyre::Report> {
    match handle.await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(err),
        Err(e) => Err(e).wrap_err_with(|| "handling failed"),
    }
}

async fn read(
    api_info: ApiInfo,
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

                handle_msg(&msg, &api_info).await?;
            }
            Err(e) => println!("{e}"),
        }
    }
    Ok(())
}

async fn handle_msg(msg: &str, api_info: &ApiInfo) -> Result<(), eyre::Report> {
    let json_msg = match serde_json::from_str::<WsEventSub>(&msg) {
        Ok(json_msg) => json_msg,
        Err(e) => {
            panic!("eventsub json Error: {} \n\n Message: {}", e, msg);
        }
    };

    let http_client = reqwest::Client::new();

    if let Some(session_id) = json_msg.payload.session {
        if let Ok(_) = http_client
            .post("https://api.twitch.tv/helix/eventsub/subscriptions")
            .bearer_auth(api_info.twitch_oauth.clone())
            .header("Client-Id", api_info.client_id.clone())
            .json(&online_event(session_id.id.clone()))
            .send()
            .await?
            .text()
            .await
        {
            println!("online sub\n");
        }

        if let Ok(_) = http_client
            .post("https://api.twitch.tv/helix/eventsub/subscriptions")
            .bearer_auth(std::env::var("TWITCH_OAUTH").expect("twitch oauth token"))
            .header("Client-Id", api_info.client_id.clone())
            .json(&offline_event(session_id.id))
            .send()
            .await?
            .text()
            .await
        {
            println!("offline sub\n");
        }
        // let response = serde_json::from_str(&response);
    } else if let Some(subscription) = json_msg.payload.subscription {
        println!("got {:?} event", subscription.r#type);

        if subscription.r#type == "stream.online" {
            match http_client
                .get("https://api.twitch.tv/helix/streams?user_id=143306668")
                .bearer_auth(api_info.twitch_oauth.clone())
                .header("Client-Id", api_info.client_id.clone())
                .send()
                .await?
                .json::<TwitchApiResponse>()
                .await
            {
                Ok(res) => {
                    send_notification(api_info, &res.data[0].title, &res.data[0].game_name).await?;
                }

                Err(e) => println!("{e}\n"),
            }
        }
    }

    Ok(())
}

// Sends live status to clients.
#[allow(unused)]
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
