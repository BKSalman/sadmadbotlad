use std::fs;

use crate::FrontEndEvent;
use crate::{discord::online_notification, ApiInfo};
use eyre::WrapErr;
use futures_util::{SinkExt, StreamExt};
use reqwest::{StatusCode, Url};
use serde_json::{json, Value};
use tokio_retry::strategy::ExponentialBackoff;
use tokio_retry::Retry;
use tokio_tungstenite::{connect_async, tungstenite::Message};

pub async fn eventsub(
    front_end_event_sender: tokio::sync::broadcast::Sender<FrontEndEvent>,
) -> Result<(), eyre::Report> {
    Retry::spawn(ExponentialBackoff::from_millis(100).take(5), || {
        read(front_end_event_sender.clone())
    })
    .await
    .with_context(|| "eventsub:: read websocket")?;

    Ok(())
}

async fn read(
    front_end_event_sender: tokio::sync::broadcast::Sender<FrontEndEvent>,
) -> Result<(), eyre::Report> {
    let (socket, _) =
        connect_async(Url::parse("wss://eventsub-beta.wss.twitch.tv/ws").expect("Url parsed"))
            .await
            .wrap_err_with(|| "Couldn't connect to eventsub websocket")?;

    let (mut sender, mut receiver) = socket.split();

    let api_info = ApiInfo::new().await?;

    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Ping(ping)) => {
                // println!("eventsub:: ping");
                sender.send(Message::Pong(ping)).await?;
            }
            Ok(msg) => {
                let msg = msg.to_string();
                if msg.contains("connection unused") {
                    println!("{msg}");
                    return Err(eyre::Report::msg("connection unsed... disconnecting"));
                }

                let json_lossy = match serde_json::from_str::<Value>(&msg) {
                    Ok(json_msg) => json_msg,
                    Err(e) => {
                        panic!("eventsub:: json Error: {} \n Message: {}", e, msg);
                    }
                };

                if let Some(session) = json_lossy["payload"]["session"].as_object() {
                    if session["status"] == "reconnecting" {
                        println!("Reconnecting eventsub");

                        let (socket, _) = connect_async(
                            Url::parse(&session["reconnect_url"].as_str().expect("reconnect url"))
                                .expect("Parsed URL"),
                        )
                        .await?;

                        let (new_sender, new_receiver) = socket.split();

                        sender = new_sender;

                        receiver = new_receiver;

                        continue;
                    }

                    let session_id = session["id"].as_str().expect("session id");

                    online_eventsub(&api_info, &session_id).await?;

                    offline_eventsub(&api_info, &session_id).await?;

                    follow_eventsub(&api_info, &session_id).await?;

                    raid_eventsub(&api_info, &session_id).await?;

                    subscribe_eventsub(&api_info, &session_id).await?;

                    println!("Subscribed to eventsubs");
                } else if let Some(subscription) =
                    &json_lossy["payload"]["subscription"].as_object()
                {
                    let sub_type = subscription["type"].as_str().expect("sub type");

                    println!("got {:?} event", sub_type);

                    match sub_type {
                        "stream.online" => {
                            online_notification(&api_info).await?;
                        }
                        "channel.follow" => {
                            let follower = json_lossy["payload"]["event"]["user_name"]
                                .as_str()
                                .expect("follow username")
                                .to_string();
                            write_recent("follow", &follower)?;
                            front_end_event_sender.send(FrontEndEvent::Follow { follower })?;
                        }
                        "channel.raid" => {
                            let from = json_lossy["payload"]["event"]["from_broadcaster_user_name"]
                                .as_str()
                                .expect("from_broadcaster_user_name")
                                .to_string();
                            write_recent("raid", &from)?;
                            front_end_event_sender.send(FrontEndEvent::Raid { from })?;
                        }
                        "channel.subscribe" => {
                            let subscriber = json_lossy["payload"]["event"]["user_name"]
                                .as_str()
                                .expect("user_name")
                                .to_string();
                            write_recent("sub", &subscriber)?;
                            front_end_event_sender.send(FrontEndEvent::Subscribe { subscriber })?;
                        }
                        _ => {}
                    }
                    // else if subscription.r#type == "stream.offline" {
                    // make it change the "started at" to "ended"
                    // }
                }
            }
            Err(e) => println!("{e}"),
        }
    }
    Ok(())
}

async fn offline_eventsub(api_info: &ApiInfo, session: &str) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    let res = http_client
        .post("https://api.twitch.tv/helix/eventsub/subscriptions")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .json(&json!({
            "type": "stream.offline",
            "version": "1",
            "condition": {
                "broadcaster_user_id": "143306668"
            },
            "transport": {
                "method": "websocket",
                "session_id": session
            }
        }))
        .send()
        .await?;

    if res.status() == StatusCode::UNAUTHORIZED {
        return Err(eyre::eyre!("offline_eventsub:: unauthorized"));
    }

    Ok(())
}

async fn online_eventsub(api_info: &ApiInfo, session: &str) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    let res = http_client
        .post("https://api.twitch.tv/helix/eventsub/subscriptions")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .json(&json!({
            "type": "stream.online",
            "version": "1",
            "condition": {
                "broadcaster_user_id": "143306668" //110644052
            },
            "transport": {
                "method": "websocket",
                "session_id": session
            }
        }))
        .send()
        .await?;

    if res.status() == StatusCode::UNAUTHORIZED {
        return Err(eyre::eyre!("online_eventsub:: unauthorized"));
    }

    Ok(())
}

async fn follow_eventsub(api_info: &ApiInfo, session: &str) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    let res = http_client
        .post("https://api.twitch.tv/helix/eventsub/subscriptions")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .json(&json!({
            "type": "channel.follow",
            "version": "1",
            "condition": {
                "broadcaster_user_id": "143306668" //110644052
            },
            "transport": {
                "method": "websocket",
                "session_id": session
            }
        }))
        .send()
        .await?;

    if res.status() == StatusCode::UNAUTHORIZED {
        return Err(eyre::eyre!("follow_eventsub:: unauthorized"));
    }

    Ok(())
}

async fn raid_eventsub(api_info: &ApiInfo, session: &str) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    let res = http_client
        .post("https://api.twitch.tv/helix/eventsub/subscriptions")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .json(&json!({
            "type": "channel.raid",
            "version": "1",
            "condition": {
                "to_broadcaster_user_id": "143306668" //110644052
            },
            "transport": {
                "method": "websocket",
                "session_id": session
            }
        }))
        .send()
        .await?;

    if res.status() == StatusCode::UNAUTHORIZED {
        return Err(eyre::eyre!("raid_eventsub:: unauthorized"));
    }

    Ok(())
}

async fn subscribe_eventsub(api_info: &ApiInfo, session: &str) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    let res = http_client
        .post("https://api.twitch.tv/helix/eventsub/subscriptions")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .json(&json!({
            "type": "channel.subscribe",
            "version": "1",
            "condition": {
                "broadcaster_user_id": "143306668" //110644052
            },
            "transport": {
                "method": "websocket",
                "session_id": session
            }
        }))
        .send()
        .await?;

    if res.status() == StatusCode::UNAUTHORIZED {
        return Err(eyre::eyre!("subscribe_eventsub:: unauthorized"));
    }

    Ok(())
}

fn write_recent(sub_type: &str, arg: impl Into<String>) -> Result<(), eyre::Report> {
    fs::write(
        format!("recent_{}.txt", sub_type),
        format!("recent {}: {}", sub_type, arg.into()),
    )?;
    Ok(())
}