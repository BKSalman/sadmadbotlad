use std::fs;
use std::sync::Arc;

use crate::{discord::online_notification, ApiInfo};
use crate::{Alert, AlertEventType, APP};
use eyre::WrapErr;
use futures_util::{SinkExt, StreamExt};
use reqwest::{StatusCode, Url};
use serde_json::{json, Value};
use tokio_tungstenite::{connect_async, tungstenite::Message};

pub async fn eventsub(api_info: Arc<ApiInfo>) -> Result<(), eyre::Report> {
    read(api_info).await?;

    Ok(())
}

async fn read(api_info: Arc<ApiInfo>) -> Result<(), eyre::Report> {
    let alerts_sender = APP.alerts_sender.clone();

    let (socket, _) =
        connect_async(Url::parse("wss://eventsub-beta.wss.twitch.tv/ws").expect("Url parsed"))
            .await
            .wrap_err_with(|| "Couldn't connect to eventsub websocket")?;

    let (mut sender, mut receiver) = socket.split();

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
                            Url::parse(session["reconnect_url"].as_str().expect("reconnect url"))
                                .expect("Parsed URL"),
                        )
                        .await?;

                        let (new_sender, new_receiver) = socket.split();

                        sender = new_sender;

                        receiver = new_receiver;

                        continue;
                    }

                    let session_id = session["id"].as_str().expect("session id");

                    online_eventsub(&api_info, session_id).await?;

                    offline_eventsub(&api_info, session_id).await?;

                    follow_eventsub(&api_info, session_id).await?;

                    raid_eventsub(&api_info, session_id).await?;

                    subscribe_eventsub(&api_info, session_id).await?;

                    resubscribe_eventsub(&api_info, session_id).await?;

                    giftsub_eventsub(&api_info, session_id).await?;

                    rewards_eventsub(&api_info, session_id).await?;

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
                            alerts_sender.send(Alert {
                                new: true,
                                alert_type: AlertEventType::Follow { follower },
                            })?;
                        }
                        "channel.raid" => {
                            let from = json_lossy["payload"]["event"]["from_broadcaster_user_name"]
                                .as_str()
                                .expect("from_broadcaster_user_name")
                                .to_string();
                            let viewers = json_lossy["payload"]["event"]["viewers"]
                                .as_u64()
                                .expect("viewers");

                            write_recent("raid", &from)?;
                            alerts_sender.send(Alert {
                                new: true,
                                alert_type: AlertEventType::Raid { from, viewers },
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
                                    alerts_sender.send(Alert {
                                        new: true,
                                        alert_type: AlertEventType::Gifted {
                                            gifted: subscriber,
                                            tier,
                                        },
                                    })?;
                                }
                                Some(false) => {
                                    let alert = AlertEventType::Subscribe { subscriber, tier };
                                    println!("Sub event:: {alert:#?}");
                                    // front_end_event_sender.send(Alert {
                                    //     new: true,
                                    //     alert_type: AlertEventType::Subscribe { subscriber, tier },
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
                            if subscribed_for > 1 {
                                alerts_sender.send(Alert {
                                    new: true,
                                    alert_type: AlertEventType::ReSubscribe {
                                        subscriber,
                                        tier,
                                        subscribed_for,
                                        streak,
                                    },
                                })?;
                                continue;
                            }

                            alerts_sender.send(Alert {
                                new: true,
                                alert_type: AlertEventType::Subscribe { subscriber, tier },
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

                            alerts_sender.send(Alert {
                                new: true,
                                alert_type: AlertEventType::GiftSub {
                                    gifter,
                                    total,
                                    tier,
                                },
                            })?;
                        }
                        "channel.channel_points_custom_reward_redemption.add" => {
                            let redeemer = json_lossy["payload"]["event"]["user_name"]
                                .as_str()
                                .expect("user_name")
                                .to_string();

                            let reward_title = json_lossy["payload"]["event"]["reward"]["title"]
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

    if res.status() != StatusCode::ACCEPTED {
        return Err(eyre::eyre!(
            "offline:: status: {} message: {}",
            res.status(),
            res.text().await?
        ));
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

    if res.status() != StatusCode::ACCEPTED {
        return Err(eyre::eyre!(
            "online:: status: {} message: {}",
            res.status(),
            res.text().await?
        ));
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

    if res.status() != StatusCode::ACCEPTED {
        return Err(eyre::eyre!(
            "follow:: status: {} message: {}",
            res.status(),
            res.text().await?
        ));
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

    if res.status() != StatusCode::ACCEPTED {
        return Err(eyre::eyre!(
            "raid:: status: {} message: {}",
            res.status(),
            res.text().await?
        ));
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

    if res.status() != StatusCode::ACCEPTED {
        return Err(eyre::eyre!(
            "subscribe:: status: {} message: {}",
            res.status(),
            res.text().await?
        ));
    }

    Ok(())
}

async fn resubscribe_eventsub(api_info: &ApiInfo, session: &str) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

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

async fn giftsub_eventsub(api_info: &ApiInfo, session: &str) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

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

async fn rewards_eventsub(api_info: &ApiInfo, session: &str) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

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
