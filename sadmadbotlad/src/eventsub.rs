use std::fs;
use std::sync::Arc;

use crate::db::{DatabaseError, Store};
use crate::discord::{offline_notification, online_notification, DiscordError};
use crate::twitch::{TwitchApiResponse, TwitchError, TwitchTokenMessages};
use crate::{Alert, AlertEventType, ApiInfo};
use chrono::ParseError;
use futures_util::{SinkExt, StreamExt};
use reqwest::{StatusCode, Url};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(thiserror::Error, Debug)]
pub enum EventsubError {
    #[error(transparent)]
    WsError(#[from] tokio_tungstenite::tungstenite::error::Error),

    #[error("connection unsed... disconnecting")]
    ConnectionUnsed,

    #[error(transparent)]
    TwitchTokenSendError(#[from] tokio::sync::mpsc::error::SendError<TwitchTokenMessages>),

    #[error(transparent)]
    AlertSendError(#[from] tokio::sync::broadcast::error::SendError<Alert>),

    #[error(transparent)]
    DateParseError(#[from] ParseError),

    #[error(transparent)]
    TwitchError(#[from] TwitchError),

    #[error(transparent)]
    DiscordError(#[from] DiscordError),

    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error(transparent)]
    DatabaseError(#[from] DatabaseError),

    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
}

pub async fn eventsub(
    alerts_sender: tokio::sync::broadcast::Sender<Alert>,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    api_info: Arc<ApiInfo>,
    store: Arc<Store>,
) -> Result<(), EventsubError> {
    read(alerts_sender, token_sender, api_info, store).await?;

    Ok(())
}

async fn read(
    alerts_sender: tokio::sync::broadcast::Sender<Alert>,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    api_info: Arc<ApiInfo>,
    store: Arc<Store>,
) -> Result<(), EventsubError> {
    let mut connection_url = String::from("wss://eventsub.wss.twitch.tv/ws");
    let mut original_connection = true;

    'restart: loop {
        let (socket, _) = connect_async(Url::parse(&connection_url).expect("Url parsed")).await?;

        let (mut sender, mut receiver) = socket.split();

        let mut discord_msg_id = String::new();
        let mut title = String::new();
        let mut game_name = String::new();

        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Ping(ping)) => {
                    // println!("eventsub:: ping");
                    sender.send(Message::Pong(ping)).await?;
                }
                Ok(Message::Close(reason)) => {
                    if let Some(close_frame) = &reason {
                        match close_frame.code {
                            CloseCode::Reserved(code) => {
                                if code == 4004 {
                                    original_connection = false;
                                    continue 'restart;
                                }
                            }
                            CloseCode::Library(code) => {
                                if code == 4002 {
                                    println!("websocket connection closed: {reason:#?}");
                                    original_connection = true;
                                    continue 'restart;
                                }
                            }
                            _ => panic!("websocket connection closed: {reason:#?}"),
                        }
                    }
                    panic!("websocket connection closed: {reason:#?}");
                }
                Ok(msg) => {
                    let msg = msg.to_string();
                    if msg.contains("connection unused") {
                        println!("{msg}");
                        return Err(EventsubError::ConnectionUnsed);
                    }

                    let json_lossy = match serde_json::from_str::<Value>(&msg) {
                        Ok(json_msg) => json_msg,
                        Err(e) => {
                            panic!("eventsub:: json Error: {} \n Message: {}", e, msg);
                        }
                    };

                    if let Some(session) = json_lossy["payload"]["session"].as_object() {
                        let status = session["status"].as_str().expect("str");
                        match status {
                            "reconnecting" => {
                                println!("Reconnecting eventsub");

                                connection_url = session["reconnect_url"]
                                    .as_str()
                                    .expect("reconnect url")
                                    .to_string();
                            }
                            "connected" => {
                                if !original_connection {
                                    continue;
                                }

                                let session_id = session["id"].as_str().expect("session id");

                                online_eventsub(token_sender.clone(), session_id).await?;

                                offline_eventsub(token_sender.clone(), session_id).await?;

                                follow_eventsub(token_sender.clone(), session_id).await?;

                                raid_eventsub(token_sender.clone(), session_id).await?;

                                subscribe_eventsub(token_sender.clone(), session_id).await?;

                                resubscribe_eventsub(token_sender.clone(), session_id).await?;

                                giftsub_eventsub(token_sender.clone(), session_id).await?;

                                rewards_eventsub(token_sender.clone(), session_id).await?;

                                cheers_eventsub(token_sender.clone(), session_id).await?;

                                println!("Subscribed to eventsubs");
                            }
                            _ => println!("status: {status}"),
                        }
                    } else if let Some(subscription) =
                        &json_lossy["payload"]["subscription"].as_object()
                    {
                        let sub_type = subscription["type"].as_str().expect("sub type");

                        println!("got {:?} event", sub_type);

                        match sub_type {
                            "stream.online" => {
                                let http_client = reqwest::Client::new();

                                let (send, recv) = oneshot::channel();

                                token_sender.send(TwitchTokenMessages::GetToken(send))?;

                                let Ok(twitch_api_info) = recv.await else {
        return Err(EventsubError::TwitchError(TwitchError::TokenError));
                                };

                                let res = http_client
                                    .get("https://api.twitch.tv/helix/streams?user_login=sadmadladsalman")
                                    .bearer_auth(twitch_api_info.twitch_access_token.clone())
                                    .header("Client-Id", twitch_api_info.client_id.clone())
                                    .send()
                                    .await?;

                                if !res.status().is_success() {
                                    return Err(EventsubError::TwitchError(
                                        TwitchError::TwitchApiError {
                                            request_name: String::from("user_id"),
                                            status: res.status(),
                                            message: res.text().await.expect("reponse text"),
                                        },
                                    ));
                                }

                                let res = res.json::<TwitchApiResponse>().await?;

                                let Some(data) = &res.data.iter().nth(0) else {
                                    println!("twitch fucked the data up");
                                    continue;
                                };

                                let timestamp =
                                    chrono::DateTime::parse_from_rfc3339(&data.started_at)?
                                        .timestamp();

                                title = res.data[0].title.clone();

                                game_name = res.data[0].game_name.clone();

                                discord_msg_id =
                                    online_notification(&title, &game_name, timestamp, &api_info)
                                        .await?;
                            }
                            "stream.offline" => {
                                if title.is_empty() || game_name.is_empty() {
                                    continue;
                                }
                                offline_notification(
                                    &title,
                                    &game_name,
                                    &api_info,
                                    &discord_msg_id,
                                )
                                .await?;
                            }
                            "channel.follow" => {
                                let follower = json_lossy["payload"]["event"]["user_name"]
                                    .as_str()
                                    .expect("follow username")
                                    .to_string();

                                write_recent("follow", &follower)?;

                                let alert = AlertEventType::Follow { follower };

                                let res = store.new_event(alert.clone()).await?;

                                println!("added {sub_type} event to db {res:#?}");

                                alerts_sender.send(Alert {
                                    new: true,
                                    r#type: alert,
                                })?;
                            }
                            "channel.raid" => {
                                let from = json_lossy["payload"]["event"]
                                    ["from_broadcaster_user_name"]
                                    .as_str()
                                    .expect("from_broadcaster_user_name")
                                    .to_string();
                                let viewers = json_lossy["payload"]["event"]["viewers"]
                                    .as_u64()
                                    .expect("viewers");

                                write_recent("raid", &from)?;

                                let alert = AlertEventType::Raid { from, viewers };

                                let res = store.new_event(alert.clone()).await?;

                                println!("added {sub_type} event to db {res:#?}");

                                alerts_sender.send(Alert {
                                    new: true,
                                    r#type: alert,
                                })?;
                            }
                            "channel.subscribe" => {
                                let subscriber = json_lossy["payload"]["event"]["user_name"]
                                    .as_str()
                                    .expect("user_name")
                                    .to_string();

                                let tier = {
                                    let long_tier = json_lossy["payload"]["event"]["tier"]
                                        .as_str()
                                        .expect("tier");
                                    if long_tier == "Prime" {
                                        long_tier.to_string()
                                    } else {
                                        long_tier.chars().next().expect("first char").to_string()
                                    }
                                };

                                write_recent("sub", &subscriber)?;
                                match json_lossy["payload"]["event"]["is_gift"].as_bool() {
                                    Some(true) => {
                                        let alert = AlertEventType::GiftedSub {
                                            gifted: subscriber,
                                            tier,
                                        };

                                        let res = store.new_event(alert.clone()).await?;

                                        println!("added {sub_type} event to db {res:#?}");

                                        alerts_sender.send(Alert {
                                            new: true,
                                            r#type: alert,
                                        })?;
                                    }
                                    Some(false) => {
                                        let alert = AlertEventType::Subscribe { subscriber, tier };
                                        println!("Sub event:: {alert:#?}");

                                        // this already gets sent as a sub message
                                        // I want to get events only when the subscriber
                                        // chooses to send the message

                                        // front_end_event_sender.send(Alert {
                                        //     new: true,
                                        //     r#type: AlertEventType::Subscribe { subscriber, tier },
                                        // })?;
                                    }
                                    None => {}
                                }
                            }
                            "channel.subscription.message" => {
                                let subscriber = json_lossy["payload"]["event"]["user_name"]
                                    .as_str()
                                    .expect("user_name")
                                    .to_string();
                                write_recent("sub", &subscriber)?;

                                let tier = json_lossy["payload"]["event"]["tier"]
                                    .as_str()
                                    .expect("tier")
                                    .chars()
                                    .next()
                                    .expect("first char")
                                    .to_string();

                                let subscribed_for = json_lossy["payload"]["event"]
                                    ["cumulative_months"]
                                    .as_u64()
                                    .expect("cumulative_months");

                                let streak = json_lossy["payload"]["event"]["streak_months"]
                                    .as_u64()
                                    .expect("streak months");

                                write_recent("sub", &subscriber)?;

                                let alert = if subscribed_for > 1 {
                                    AlertEventType::ReSubscribe {
                                        subscriber,
                                        tier,
                                        subscribed_for,
                                        streak,
                                    }
                                } else {
                                    AlertEventType::Subscribe { subscriber, tier }
                                };

                                let res = store.new_event(alert.clone()).await?;

                                println!("added {sub_type} event to db {res:#?}");

                                alerts_sender.send(Alert {
                                    new: true,
                                    r#type: alert,
                                })?;
                            }
                            "channel.subscription.gift" => {
                                let gifter = json_lossy["payload"]["event"]["user_name"]
                                    .as_str()
                                    .expect("user_name")
                                    .to_string();

                                let tier = {
                                    let long_tier = json_lossy["payload"]["event"]["tier"]
                                        .as_str()
                                        .expect("tier");
                                    if long_tier != "Prime" {
                                        long_tier.chars().next().expect("first char").to_string()
                                    } else {
                                        long_tier.to_string()
                                    }
                                };

                                let total = json_lossy["payload"]["event"]["total"]
                                    .as_u64()
                                    .expect("total");

                                let alert = AlertEventType::GiftSub {
                                    gifter,
                                    total,
                                    tier,
                                };

                                let res = store.new_event(alert.clone()).await?;

                                println!("added {sub_type} event to db {res:#?}");

                                alerts_sender.send(Alert {
                                    new: true,
                                    r#type: alert,
                                })?;
                            }
                            "channel.cheer" => {
                                let message = json_lossy["payload"]["event"]["message"]
                                    .as_str()
                                    .expect("message")
                                    .to_string();

                                let is_anonymous = json_lossy["payload"]["event"]["is_anonymous"]
                                    .as_bool()
                                    .expect("is_anonymous");

                                let cheerer = json_lossy["payload"]["event"]["user_name"]
                                    .as_str()
                                    .expect("user_name")
                                    .to_string();

                                let bits = json_lossy["payload"]["event"]["bits"]
                                    .as_u64()
                                    .expect("bits");

                                let alert = AlertEventType::Bits {
                                    message,
                                    is_anonymous,
                                    cheerer,
                                    bits,
                                };

                                let res = store.new_event(alert.clone()).await?;

                                println!("added {sub_type} event to db {res:#?}");

                                alerts_sender.send(Alert {
                                    new: true,
                                    r#type: alert,
                                })?;
                            }
                            "channel.channel_points_custom_reward_redemption.add" => {
                                let redeemer = json_lossy["payload"]["event"]["user_name"]
                                    .as_str()
                                    .expect("user_name")
                                    .to_string();

                                let reward_title = json_lossy["payload"]["event"]["reward"]
                                    ["title"]
                                    .as_str()
                                    .expect("reward title")
                                    .to_string();

                                println!("{redeemer} redeemed: {reward_title}");
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => println!("{e}"),
            }
        }
    }
}

async fn offline_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), EventsubError> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(EventsubError::TwitchError(TwitchError::TokenError));
    };

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

    if res.status() != StatusCode::ACCEPTED {
        return Err(EventsubError::TwitchError(TwitchError::TwitchApiError {
            request_name: String::from("offline"),
            status: res.status(),
            message: res.text().await.expect("response text"),
        }));
    }

    Ok(())
}

async fn cheers_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), EventsubError> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(EventsubError::TwitchError(TwitchError::TokenError));
    };

    let res = http_client
        .post("https://api.twitch.tv/helix/eventsub/subscriptions")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .json(&json!({
            "type": "channel.cheer",
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

    if res.status() != StatusCode::ACCEPTED {
        return Err(EventsubError::TwitchError(TwitchError::TwitchApiError {
            request_name: String::from("cheers"),
            status: res.status(),
            message: res.text().await.expect("response text"),
        }));
    }

    Ok(())
}

async fn online_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), EventsubError> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(EventsubError::TwitchError(TwitchError::TokenError));
    };

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

    if res.status() != StatusCode::ACCEPTED {
        return Err(EventsubError::TwitchError(TwitchError::TwitchApiError {
            request_name: String::from("online"),
            status: res.status(),
            message: res.text().await.expect("response text"),
        }));
    }

    Ok(())
}

async fn follow_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), EventsubError> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(EventsubError::TwitchError(TwitchError::TokenError));
    };

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

    if res.status() != StatusCode::ACCEPTED {
        return Err(EventsubError::TwitchError(TwitchError::TwitchApiError {
            request_name: String::from("follow"),
            status: res.status(),
            message: res.text().await.expect("response text"),
        }));
    }

    Ok(())
}

async fn raid_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), EventsubError> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(EventsubError::TwitchError(TwitchError::TokenError));
    };

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

    if res.status() != StatusCode::ACCEPTED {
        return Err(EventsubError::TwitchError(TwitchError::TwitchApiError {
            request_name: String::from("raid"),
            status: res.status(),
            message: res.text().await.expect("response text"),
        }));
    }

    Ok(())
}

async fn subscribe_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), EventsubError> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(EventsubError::TwitchError(TwitchError::TokenError));
    };

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

    if res.status() != StatusCode::ACCEPTED {
        return Err(EventsubError::TwitchError(TwitchError::TwitchApiError {
            request_name: String::from("subscribe"),
            status: res.status(),
            message: res.text().await.expect("response text"),
        }));
    }

    Ok(())
}

async fn resubscribe_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), EventsubError> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(EventsubError::TwitchError(TwitchError::TokenError));
    };

    let res = http_client
        .post("https://api.twitch.tv/helix/eventsub/subscriptions")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .json(&json!({
            "type": "channel.subscription.message",
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

    if res.status() != StatusCode::ACCEPTED {
        return Err(EventsubError::TwitchError(TwitchError::TwitchApiError {
            request_name: String::from("resubscribe"),
            status: res.status(),
            message: res.text().await.expect("response text"),
        }));
    }

    Ok(())
}

async fn giftsub_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), EventsubError> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(EventsubError::TwitchError(TwitchError::TokenError));
    };

    let res = http_client
        .post("https://api.twitch.tv/helix/eventsub/subscriptions")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .json(&json!({
            "type": "channel.subscription.gift",
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

    if res.status() != StatusCode::ACCEPTED {
        return Err(EventsubError::TwitchError(TwitchError::TwitchApiError {
            request_name: String::from("giftsub"),
            status: res.status(),
            message: res.text().await.expect("response text"),
        }));
    }

    Ok(())
}

async fn rewards_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), EventsubError> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(EventsubError::TwitchError(TwitchError::TokenError));
    };

    let res = http_client
        .post("https://api.twitch.tv/helix/eventsub/subscriptions")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .json(&json!({
            "type": "channel.channel_points_custom_reward_redemption.add",
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

    if res.status() != StatusCode::ACCEPTED {
        return Err(EventsubError::TwitchError(TwitchError::TwitchApiError {
            request_name: String::from("rewards"),
            status: res.status(),
            message: res.text().await.expect("response text"),
        }));
    }

    Ok(())
}

fn write_recent(sub_type: &str, arg: impl Into<String>) -> Result<(), EventsubError> {
    fs::write(
        format!("recents/recent_{}.txt", sub_type),
        format!("recent {}: {}", sub_type, arg.into()),
    )?;
    Ok(())
}
