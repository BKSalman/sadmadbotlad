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
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(thiserror::Error, Debug)]
pub enum EventsubError {
    #[error(transparent)]
    WsError(#[from] tokio_tungstenite::tungstenite::error::Error),

    #[error("connection unsed... disconnecting")]
    ConnectionUnused,

    #[error(transparent)]
    IrcChannelError(
        #[from] tokio::sync::mpsc::error::SendError<tokio_tungstenite::tungstenite::Message>,
    ),

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
    let (irc_sender, mut irc_receiver) = mpsc::unbounded_channel::<Message>();

    let mut connections_handlers = vec![];

    'restart: loop {
        tracing::debug!("'restart loop");

        let mut discord_msg_id = String::new();
        let mut title = String::new();
        let mut game_name = String::new();

        {
            let irc_sender = irc_sender.clone();
            connections_handlers.push(new_connection(
                "wss://eventsub.wss.twitch.tv/ws",
                irc_sender,
            ));
        }

        while let Some(msg) = irc_receiver.recv().await {
            match msg {
                Message::Close(reason) => {
                    // if let Some(close_frame) = &reason {
                    //     if let CloseCode::Reserved(code) = close_frame.code {
                    //         if code == 4004 {
                    //             continue 'restart;
                    //         }
                    //     }
                    // }

                    tracing::error!("websocket connection closed: {reason:#?}");
                    continue 'restart;
                }
                msg => {
                    let msg = msg.to_string();
                    if msg.contains("connection unused") {
                        tracing::error!("connection unused: {msg}");
                        return Err(EventsubError::ConnectionUnused);
                    }

                    let json_lossy = match serde_json::from_str::<Value>(&msg) {
                        Ok(json_msg) => json_msg,
                        Err(e) => {
                            panic!("eventsub:: json Error: {} \n Message: {}", e, msg);
                        }
                    };

                    match json_lossy["metadata"]["message_type"].as_str() {
                        // Some("session_reconnect") => {
                        //     tracing::debug!("Reconnecting eventsub");

                        //     connection_url = session["reconnect_url"]
                        //         .as_str()
                        //         .expect("reconnect url")
                        //         .to_string();
                        // }
                        Some("session_welcome") => {
                            if let Some(_reconnect_url) =
                                json_lossy["payload"]["session"]["reconnect_url"].as_str()
                            {
                                tracing::debug!("removing old connections");

                                // abort all tokio tasks but the last one
                                // to remove old connections
                                let new_handler = connections_handlers.pop().expect("new handler");

                                for handler in connections_handlers.iter() {
                                    handler.abort();
                                }

                                connections_handlers.clear();
                                connections_handlers.push(new_handler);

                                tracing::debug!(
                                    "connections after aborting: {}",
                                    connections_handlers.len()
                                );
                            }

                            if let Some(session) = json_lossy["payload"]["session"].as_object() {
                                let status = session["status"].as_str();
                                match status {
                                    Some("connected") => {
                                        let session_id =
                                            session["id"].as_str().expect("session id");

                                        subscribe(&token_sender, session_id).await?;

                                        tracing::info!("Subscribed to eventsubs");
                                    }
                                    _ => tracing::debug!("status: {status:#?}"),
                                }
                            }
                        }
                        Some("notification") => {
                            if let Some(sub_type) =
                                json_lossy["payload"]["subscription"]["type"].as_str()
                            {
                                tracing::debug!("got {:?} event", sub_type);

                                match sub_type {
                                    "stream.online" => {
                                        stream_online_event(
                                            &token_sender,
                                            &mut title,
                                            &mut game_name,
                                            &mut discord_msg_id,
                                            &api_info,
                                        )
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

                                        tracing::debug!("added {sub_type} event to db {res:#?}");

                                        if let Err(err) = alerts_sender.send(Alert {
                                            new: true,
                                            r#type: alert.clone(),
                                        }) {
                                            tracing::error!(
                                                "failed to send alert: {alert:#?} -- {err:#?}"
                                            );
                                        }
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

                                        tracing::debug!("added {sub_type} event to db {res:#?}");

                                        alerts_sender.send(Alert {
                                            new: true,
                                            r#type: alert,
                                        })?;
                                    }
                                    "channel.subscribe" => {
                                        channel_subscribe_event(
                                            &json_lossy,
                                            &store,
                                            &alerts_sender,
                                        )
                                        .await?;
                                    }
                                    "channel.subscription.message" => {
                                        channel_subscription_message_event(
                                            &json_lossy,
                                            &store,
                                            &alerts_sender,
                                        )
                                        .await?
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
                                                long_tier
                                                    .chars()
                                                    .next()
                                                    .expect("first char")
                                                    .to_string()
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

                                        tracing::debug!("added {sub_type} event to db {res:#?}");

                                        alerts_sender.send(Alert {
                                            new: true,
                                            r#type: alert,
                                        })?;
                                    }
                                    "channel.cheer" => {
                                        channel_cheer_event(&json_lossy, &store, &alerts_sender)
                                            .await?
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

                                        tracing::debug!("{redeemer} redeemed: {reward_title}");
                                    }
                                    _ => todo!(),
                                }
                            }
                        }
                        Some("session_reconnect") => {
                            if let Some(reconnect_url) =
                                json_lossy["payload"]["reconnect_url"].as_str()
                            {
                                {
                                    let irc_sender = irc_sender.clone();
                                    connections_handlers
                                        .push(new_connection(reconnect_url, irc_sender));
                                }
                            }
                        }
                        Some("session_keepalive") => {}
                        Some(_) => todo!(),
                        None => todo!(),
                    };
                }
            }
        }
    }
}

fn new_connection(
    connection_url: &str,
    irc_sender: mpsc::UnboundedSender<Message>,
) -> tokio::task::JoinHandle<Result<(), EventsubError>> {
    tracing::debug!("new connection");
    let connection_url = connection_url.to_string();
    tokio::spawn(async move {
        let (mut sender, mut receiver) =
            connect_async(Url::parse(&connection_url).expect("Url parsed"))
                .await?
                .0
                .split();

        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Ping(ping)) => {
                    // tracing::debug!("eventsub:: ping {ping:?}");
                    sender.send(Message::Pong(ping)).await?;
                }
                Ok(msg) => irc_sender.send(msg)?,
                Err(err) => {
                    tracing::error!("eventsub websocket error: {err}");
                }
            }
        }

        Ok::<_, EventsubError>(())
    })
}

async fn channel_cheer_event(
    json_lossy: &Value,
    store: &Arc<Store>,
    alerts_sender: &tokio::sync::broadcast::Sender<Alert>,
) -> Result<(), EventsubError> {
    let message = (*json_lossy)["payload"]["event"]["message"]
        .as_str()
        .expect("message")
        .to_string();

    let is_anonymous = (*json_lossy)["payload"]["event"]["is_anonymous"]
        .as_bool()
        .expect("is_anonymous");

    let cheerer = (*json_lossy)["payload"]["event"]["user_name"]
        .as_str()
        .expect("user_name")
        .to_string();

    let bits = (*json_lossy)["payload"]["event"]["bits"]
        .as_u64()
        .expect("bits");

    let alert = AlertEventType::Bits {
        message,
        is_anonymous,
        cheerer,
        bits,
    };

    let res = store.new_event(alert.clone()).await?;

    tracing::debug!("added {alert:#?} event to db {res:#?}");

    alerts_sender.send(Alert {
        new: true,
        r#type: alert,
    })?;

    Ok(())
}

async fn channel_subscription_message_event(
    json_lossy: &Value,
    store: &Arc<Store>,
    alerts_sender: &tokio::sync::broadcast::Sender<Alert>,
) -> Result<(), EventsubError> {
    let subscriber = (*json_lossy)["payload"]["event"]["user_name"]
        .as_str()
        .expect("user_name")
        .to_string();

    write_recent("sub", &subscriber)?;

    let tier = (*json_lossy)["payload"]["event"]["tier"]
        .as_str()
        .expect("tier")
        .chars()
        .next()
        .expect("first char")
        .to_string();

    let subscribed_for = (*json_lossy)["payload"]["event"]["cumulative_months"]
        .as_u64()
        .expect("cumulative_months");

    let streak = (*json_lossy)["payload"]["event"]["streak_months"]
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

    tracing::debug!("added {:#?} event to db {res:#?}", alert);

    alerts_sender.send(Alert {
        new: true,
        r#type: alert,
    })?;

    Ok(())
}

async fn stream_online_event(
    token_sender: &mpsc::UnboundedSender<TwitchTokenMessages>,
    title: &mut String,
    game_name: &mut String,
    discord_msg_id: &mut String,
    api_info: &Arc<ApiInfo>,
) -> Result<(), EventsubError> {
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
        return Err(EventsubError::TwitchError(TwitchError::TwitchApiError {
            request_name: String::from("user_id"),
            status: res.status(),
            message: res.text().await.expect("reponse text"),
        }));
    }

    let res = res.json::<TwitchApiResponse>().await?;
    let Some(data) = &res.data.get(0) else {
        return Err(EventsubError::TwitchError(TwitchError::FuckedUp));
    };

    let timestamp = chrono::DateTime::parse_from_rfc3339(&data.started_at)?.timestamp();

    *title = res.data[0].title.clone();
    *game_name = res.data[0].game_name.clone();
    *discord_msg_id = online_notification(&*title, &*game_name, timestamp, api_info).await?;

    Ok(())
}

async fn channel_subscribe_event(
    json_lossy: &Value,
    store: &Arc<Store>,
    alerts_sender: &tokio::sync::broadcast::Sender<Alert>,
) -> Result<(), EventsubError> {
    let subscriber = (*json_lossy)["payload"]["event"]["user_name"]
        .as_str()
        .expect("user_name")
        .to_string();

    let tier = {
        let long_tier = (*json_lossy)["payload"]["event"]["tier"]
            .as_str()
            .expect("tier");
        if long_tier == "Prime" {
            long_tier.to_string()
        } else {
            long_tier.chars().next().expect("first char").to_string()
        }
    };

    write_recent("sub", &subscriber)?;

    match (*json_lossy)["payload"]["event"]["is_gift"].as_bool() {
        Some(true) => {
            let alert = AlertEventType::GiftedSub {
                gifted: subscriber,
                tier,
            };

            let res = store.new_event(alert.clone()).await?;

            tracing::debug!("added {:#?} event to db {res:#?}", alert);

            alerts_sender.send(Alert {
                new: true,
                r#type: alert,
            })?;
        }
        Some(false) => {
            let alert = AlertEventType::Subscribe { subscriber, tier };
            tracing::info!("Sub event:: {alert:#?}");

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

    Ok(())
}

async fn subscribe(
    token_sender: &mpsc::UnboundedSender<TwitchTokenMessages>,
    session_id: &str,
) -> Result<(), EventsubError> {
    online_eventsub(token_sender.clone(), session_id).await?;
    offline_eventsub(token_sender.clone(), session_id).await?;
    follow_eventsub(token_sender.clone(), session_id).await?;
    raid_eventsub(token_sender.clone(), session_id).await?;
    subscribe_eventsub(token_sender.clone(), session_id).await?;
    resubscribe_eventsub(token_sender.clone(), session_id).await?;
    giftsub_eventsub(token_sender.clone(), session_id).await?;
    rewards_eventsub(token_sender.clone(), session_id).await?;
    cheers_eventsub(token_sender.clone(), session_id).await?;

    Ok(())
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
            "version": "2",
            "condition": {
                "broadcaster_user_id": "143306668", //110644052
                "moderator_user_id": "143306668"
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
