use crate::{
    discord::online_notification,
    twitch::TwitchApiResponse,
    ApiInfo,
};
use crate::{flatten, FrontEndEvent};
use eyre::WrapErr;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use reqwest::Url;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

pub async fn eventsub(
    front_end_event_sender: tokio::sync::broadcast::Sender<FrontEndEvent>,
) -> Result<(), eyre::Report> {
    println!("Starting eventsub");
    let api_info = ApiInfo::new()?;

    let (socket, _) =
        connect_async(Url::parse("wss://eventsub-beta.wss.twitch.tv/ws").expect("Url parsed"))
            .await?;

    let (sender, receiver) = socket.split();

    if let Err(e) = tokio::try_join!(
        // flatten(tokio::spawn(write(sender, watch))),
        flatten(tokio::spawn(read(
            front_end_event_sender,
            api_info,
            receiver,
            sender
        )))
    )
    .wrap_err_with(|| "in stream join")
    {
        eprintln!("socket failed {e}")
    }

    Ok(())
}

async fn read(
    front_end_event_sender: tokio::sync::broadcast::Sender<FrontEndEvent>,
    api_info: ApiInfo,
    mut recv: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    mut sender: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
) -> Result<(), eyre::Report> {
    while let Some(msg) = recv.next().await {
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

                // let json_msg = match serde_json::from_str::<WsEventSub>(&msg) {
                //     Ok(json_msg) => json_msg,
                //     Err(e) => {
                //         panic!("eventsub json Error: {} \n\n Message: {}", e, msg);
                //     }
                // };

                let json_lossy = match serde_json::from_str::<Value>(&msg) {
                    Ok(json_msg) => json_msg,
                    Err(e) => {
                        panic!("eventsub:: json Error: {} \n\n Message: {}", e, msg);
                    }
                };

                if let Some(session) = json_lossy["payload"]["session"].as_object() {
                    if session["status"] == "reconnecting" {
                        println!("Reconnecting eventsub");

                        let (socket, _) = connect_async(
                            Url::parse(&session["reconnect_url"].as_str().expect("reconnect url"))
                                .expect("Url parsed"),
                        )
                        .await?;

                        let (send, receiver) = socket.split();

                        sender = send;

                        recv = receiver;

                        continue;
                    }

                    let session_id = session["id"].as_str().expect("session id");

                    if let Ok(_) = offline_eventsub(&api_info, &session_id).await {
                        println!("offline sub");
                    }
                    if let Ok(_) = online_eventsub(&api_info, &session_id).await {
                        println!("online sub");
                    }
                    if let Ok(_) = follow_eventsub(&api_info, &session_id).await {
                        println!("follow sub");
                    }
                    if let Ok(_) = raid_eventsub(&api_info, &session_id).await {
                        println!("raid sub");
                    }
                } else if let Some(subscription) =
                    &json_lossy["payload"]["subscription"].as_object()
                {
                    let sub_type = subscription["type"].as_str().expect("sub type");

                    println!("got {:?} event", sub_type);
                    // println!("{:#?}", json_msg);

                    match sub_type {
                        "stream.online" => {
                            online_event(&api_info).await?;
                        }
                        "channel.follow" => {
                            front_end_event_sender.send(FrontEndEvent::Follow {
                                follower: json_lossy["payload"]["event"]["user_name"]
                                    .as_str()
                                    .expect("follow username")
                                    .to_string(),
                            })?;
                        }
                        "channel.raid" => {
                            front_end_event_sender.send(FrontEndEvent::Raid {
                                from: json_lossy["payload"]["event"]["from_broadcaster_user_name"]
                                    .as_str()
                                    .expect("from_broadcaster_user_name")
                                    .to_string(),
                            })?;
                        }
                        _ => {}
                    }
                    // else if subscription.r#type == "stream.offline" {
                    //     match http_client
                    //         .get("https://api.twitch.tv/helix/streams?user_id=143306668")
                    //         .bearer_auth(api_info.twitch_oauth.clone())
                    //         .header("Client-Id", api_info.client_id.clone())
                    //         .send()
                    //         .await?
                    //         .json::<TwitchApiResponse>()
                    //         .await
                    //     {
                    //         Ok(res) => {
                    //             online_notification(api_info, &res.data[0].title, &res.data[0].game_name).await?;
                    //         }

                    //         Err(e) => println!("{e}\n"),
                    //     }
                    // }
                }
            }
            Err(e) => println!("{e}"),
        }
    }
    Ok(())
}

async fn online_event(api_info: &ApiInfo) -> Result<(), color_eyre::Report> {
    let http_client = reqwest::Client::new();

    match http_client
        .get("https://api.twitch.tv/helix/streams?user_id=143306668")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .send()
        .await?
        .json::<TwitchApiResponse>()
        .await
    {
        Ok(res) => {
            online_notification(api_info, &res.data[0].title, &res.data[0].game_name).await?;
        }

        Err(e) => println!("{e}\n"),
    }

    Ok(())
}

async fn offline_eventsub(api_info: &ApiInfo, session: &str) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    http_client
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

    Ok(())
}

async fn online_eventsub(api_info: &ApiInfo, session: &str) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    http_client
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

    Ok(())
}

async fn follow_eventsub(api_info: &ApiInfo, session: &str) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    http_client
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

    Ok(())
}

async fn raid_eventsub(api_info: &ApiInfo, session: &str) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    http_client
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

    Ok(())
}
