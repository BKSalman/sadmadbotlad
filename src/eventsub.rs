use crate::{
    discord::online_notification,
    twitch::{
        follow_event_body, offline_event_body, online_event_body, TwitchApiResponse, WsEventSub,
        WsSession,
    },
    ApiInfo
};
use crate::{flatten, FrontEndEvent};
use eyre::WrapErr;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use reqwest::Url;
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

pub async fn eventsub(
    front_end_event_sender: tokio::sync::mpsc::Sender<FrontEndEvent>,
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
    front_end_event_sender: tokio::sync::mpsc::Sender<FrontEndEvent>,
    api_info: ApiInfo,
    mut recv: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    mut sender: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
) -> Result<(), eyre::Report> {
    while let Some(msg) = recv.next().await {
        match msg {
            Ok(Message::Ping(ping)) => {
                // println!("ping");
                sender.send(Message::Pong(ping)).await?;
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

                if let Some(session) = json_msg.payload.session {
                    if session.status == "reconnecting" {
                        println!("Reconnecting eventsub");

                        let (socket, _) = connect_async(
                            Url::parse(&session.reconnect_url.expect("handle_msg:: reconnect url"))
                                .expect("Url parsed"),
                        )
                        .await?;

                        let (send, receiver) = socket.split();

                        sender = send;

                        recv = receiver;

                        continue;
                    }

                    if let Ok(_) = offline_eventsub(&api_info, &session).await {
                        println!("offline sub");
                    }

                    if let Ok(_) = online_eventsub(&api_info, &session).await {
                        println!("online sub");
                    }
                    if let Ok(_) = follow_eventsub(&api_info, &session).await {
                        println!("follow sub");
                    }
                } else if let Some(_) = &json_msg.payload.session {
                } else if let Some(subscription) = &json_msg.payload.subscription {
                    println!("got {:?} event", subscription.r#type);
                    // println!("{:#?}", json_msg);

                    if subscription.r#type == "stream.online" {
                        online_event(&api_info).await?;
                    } else if subscription.r#type == "channel.follow" {
                        front_end_event_sender
                            .send(FrontEndEvent::Follow {
                                follower: json_msg
                                    .payload
                                    .event
                                    .expect("follow event")
                                    .user_name
                                    .expect("follow username"),
                            })
                            .await?;
                        println!("sent to frontend handler");
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

async fn offline_eventsub(api_info: &ApiInfo, session: &WsSession) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    http_client
        .post("https://api.twitch.tv/helix/eventsub/subscriptions")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .json(&offline_event_body(session.id.clone()))
        .send()
        .await?;

    Ok(())
}

async fn online_eventsub(api_info: &ApiInfo, session: &WsSession) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    http_client
        .post("https://api.twitch.tv/helix/eventsub/subscriptions")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .json(&online_event_body(session.id.clone()))
        .send()
        .await?;

    Ok(())
}

async fn follow_eventsub(api_info: &ApiInfo, session: &WsSession) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    http_client
        .post("https://api.twitch.tv/helix/eventsub/subscriptions")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .json(&follow_event_body(session.id.clone()))
        .send()
        .await?;

    Ok(())
}
