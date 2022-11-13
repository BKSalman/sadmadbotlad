use std::error::Error;

use eyre::WrapErr;
use serde_json::json;
use tokio::{net::TcpStream, sync::watch, task::JoinHandle};
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};
use serde::Deserialize;

mod discord;
mod twitch;

use twitch::{EventSubResponse, LiveStatus};
use discord::send_notification;
use crate::twitch::{offline_event, online_event, TwitchApiResponse, WsEventSub};

#[derive(Deserialize)]
pub struct ApiInfo {
    client_id: String,
    pub twitch_oauth: String,
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

    pub async fn delete_subs(&self) -> Result<(), eyre::Report> {
        let http_client = reqwest::Client::new();

        let subs = http_client
            .get("https://api.twitch.tv/helix/eventsub/subscriptions")
            .bearer_auth(self.twitch_oauth.clone())
            .header("Client-Id", self.client_id.clone())
            .send()
            .await?
            .json::<EventSubResponse>()
            .await?;

        for sub in subs.data {
            println!("deleting sub with id {}", sub.id);

            http_client
                .delete("https://api.twitch.tv/helix/eventsub/subscriptions")
                .bearer_auth(self.twitch_oauth.clone())
                .header("Client-Id", self.client_id.clone())
                .json(&json!({
                    "id": sub.id
                }))
                .send()
                .await?;
        }

        Ok(())
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
        let response = http_client
            .post("https://api.twitch.tv/helix/eventsub/subscriptions")
            .bearer_auth(api_info.twitch_oauth.clone())
            .header("Client-Id", api_info.client_id.clone())
            .json(&online_event(session_id.id.clone()))
            .send()
            .await?
            .text()
            .await?;

        println!("{:?}", response);

        let response = http_client
            .post("https://api.twitch.tv/helix/eventsub/subscriptions")
            .bearer_auth(std::env::var("TWITCH_OAUTH").expect("twitch oauth token"))
            .header("Client-Id", api_info.client_id.clone())
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
            match http_client
                .post("https://api.twitch.tv/helix/eventsub/subscriptions")
                .bearer_auth(api_info.twitch_oauth.clone())
                .header("Client-Id", api_info.client_id.clone())
                .json(&json!({
                    "broadcaster_id": "143306668"
                }))
                .send()
                .await?
                .json::<TwitchApiResponse>()
                .await
            {
                Ok(res) => {
                    send_notification(&res.data[0].title).await?;
                }

                Err(e) => println!("{e}"),
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
