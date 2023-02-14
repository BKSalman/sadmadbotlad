use std::fs;
use std::sync::Arc;

use crate::db::Store;
use crate::discord::online_notification;
use crate::twitch::TwitchTokenMessages;
use crate::{Alert, AlertEventType, ApiInfo};
use eyre::WrapErr;
use futures_util::{SinkExt, StreamExt};
use reqwest::{StatusCode, Url};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::{connect_async, tungstenite::Message};

pub async fn eventsub(
    alerts_sender: tokio::sync::broadcast::Sender<Alert>,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    api_info: Arc<ApiInfo>,
    store: Arc<Store>,
) -> Result<(), eyre::Report> {
    read(alerts_sender, token_sender, api_info, store).await?;

    Ok(())
}

async fn read(
    alerts_sender: tokio::sync::broadcast::Sender<Alert>,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    api_info: Arc<ApiInfo>,
    store: Arc<Store>,
) -> Result<(), eyre::Report> {
    let mut connection_url = String::from("wss://eventsub-beta.wss.twitch.tv/ws");
    let mut original_connection = true;

    'restart: loop {
        let (socket, _) = connect_async(Url::parse(&connection_url).expect("Url parsed"))
            .await
            .wrap_err_with(|| "Couldn't connect to eventsub websocket")?;

        let (mut sender, mut receiver) = socket.split();

        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Ping(ping)) => {
                    // println!("eventsub:: ping");
                    sender.send(Message::Pong(ping)).await?;
                }
                Ok(Message::Close(reason)) => {
                    if let Some(close_frame) = &reason {
                        if u16::from(close_frame.code) == 4004 {
                            original_connection = false;
                            continue 'restart;
                        }
                    }
                    // TODO: maybe check if it's a minor reason then handle it
                    // otherwise just panic

                    // original_connection = true;
                    // connection_url = String::from("wss://eventsub-beta.wss.twitch.tv/ws");
                    // continue 'restart;

                    panic!("websocket connection closed: {reason:#?}");
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
                                online_notification(&api_info).await?;
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
) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(eyre::eyre!("Failed to get token"));
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
        return Err(eyre::eyre!(
            "offline:: status: {} message: {}",
            res.status(),
            res.text().await?
        ));
    }

    Ok(())
}

async fn cheers_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(eyre::eyre!("Failed to get token"));
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
        return Err(eyre::eyre!(
            "cheers:: status: {} message: {}",
            res.status(),
            res.text().await?
        ));
    }

    Ok(())
}

async fn online_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(eyre::eyre!("Failed to get token"));
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
        return Err(eyre::eyre!(
            "online:: status: {} message: {}",
            res.status(),
            res.text().await?
        ));
    }

    Ok(())
}

async fn follow_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(eyre::eyre!("Failed to get token"));
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
        return Err(eyre::eyre!(
            "follow:: status: {} message: {}",
            res.status(),
            res.text().await?
        ));
    }

    Ok(())
}

async fn raid_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(eyre::eyre!("Failed to get token"));
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
        return Err(eyre::eyre!(
            "raid:: status: {} message: {}",
            res.status(),
            res.text().await?
        ));
    }

    Ok(())
}

async fn subscribe_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(eyre::eyre!("Failed to get token"));
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
        return Err(eyre::eyre!(
            "subscribe:: status: {} message: {}",
            res.status(),
            res.text().await?
        ));
    }

    Ok(())
}

async fn resubscribe_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(eyre::eyre!("Failed to get token"));
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
        return Err(eyre::eyre!(
            "resubscribe:: status: {} message: {}",
            res.status(),
            res.text().await?
        ));
    }

    Ok(())
}

async fn giftsub_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(eyre::eyre!("Failed to get token"));
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
        return Err(eyre::eyre!(
            "resubscribe:: status: {} message: {}",
            res.status(),
            res.text().await?
        ));
    }

    Ok(())
}

async fn rewards_eventsub(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    session: &str,
) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    let (send, recv) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(send))?;

    let Ok(api_info) = recv.await else {
        return Err(eyre::eyre!("Failed to get token"));
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
        return Err(eyre::eyre!(
            "rewards:: status: {} message: {}",
            res.status(),
            res.text().await?
        ));
    }

    Ok(())
}

fn write_recent(sub_type: &str, arg: impl Into<String>) -> Result<(), eyre::Report> {
    fs::write(
        format!("recents/recent_{}.txt", sub_type),
        format!("recent {}: {}", sub_type, arg.into()),
    )?;
    Ok(())
}
