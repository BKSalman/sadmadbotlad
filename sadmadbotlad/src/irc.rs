use std::sync::Arc;

use crate::{
    event_handler::{self, event_handler, Event, IrcChat, IrcEvent, IrcWs},
    flatten,
    song_requests::{play_song, setup_mpv, SongRequest},
    ApiInfo, FrontEndEvent,
};
use eyre::WrapErr;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use tokio::{net::TcpStream, sync::mpsc::{UnboundedSender, UnboundedReceiver}};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

pub async fn irc_connect (
    front_end_events_sender: tokio::sync::broadcast::Sender<FrontEndEvent>,
    e_sender: UnboundedSender<Event>,
    e_receiver: UnboundedReceiver<Event>
) -> eyre::Result<()> {
    println!("Starting IRC");

    let (socket, _) = connect_async("wss://irc-ws.chat.twitch.tv:443").await?;

    let (ws_sender, ws_receiver) = socket.split();

    let ws_receiver = Arc::new(tokio::sync::Mutex::new(ws_receiver));


    let (song_sender, song_receiver) = tokio::sync::mpsc::channel::<SongRequest>(200);

    let e_sender_c = e_sender.clone();

    let mpv = Arc::new(setup_mpv());

    let mpv_c = mpv.clone();

    let join_handle = std::thread::spawn(move || play_song(mpv_c, song_receiver, e_sender_c));

    tokio::try_join!(
        flatten(tokio::spawn(read(e_sender, ws_receiver.clone()))),
        flatten(tokio::spawn(event_handler(
            song_sender,
            mpv,
            e_receiver,
            ws_sender,
            ws_receiver,
            front_end_events_sender,
        ))),
    )
    .wrap_err_with(|| "irc")?;

    join_handle.join().expect("join")?;

    Ok(())
}

pub async fn irc_login(
    ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    api_info: &ApiInfo,
) -> Result<(), eyre::Report> {
    let cap = Message::Text(String::from("CAP REQ :twitch.tv/commands twitch.tv/tags"));

    ws_sender.send(cap).await?;

    let pass_msg = Message::Text(format!("PASS oauth:{}", api_info.twitch_access_token));

    ws_sender.send(pass_msg).await?;

    let nick_msg = Message::Text(format!("NICK {}", api_info.user));

    ws_sender.send(nick_msg).await?;

    ws_sender
        .send(Message::Text(String::from("JOIN #sadmadladsalman")))
        .await?;

    Ok(())
}

async fn read(
    event_sender: UnboundedSender<Event>,
    ws_receiver: Arc<tokio::sync::Mutex<SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>>>,
) -> eyre::Result<()> {
    let mut locked_ws_receiver = ws_receiver.lock().await;

    while let Some(msg) = locked_ws_receiver.next().await {
        match msg {
            Ok(Message::Ping(ping)) => {
                println!("IRC WebSocket Ping {ping:?}");
                event_sender.send(Event::IrcEvent(IrcEvent::WebSocket(IrcWs::Ping(ping))))?;
            }
            Ok(Message::Text(msg)) if msg.contains("PRIVMSG") => {
                let parsed_sender = parse_sender(&msg);
                let parsed_msg = parse_message(&msg);

                if !parsed_msg.starts_with('!') {
                    continue;
                }
                
                let tags = msg.split_once(':').expect("no chat tags").0;
                // ^have a proper structure for it later, and methods to extract is_mod and is_vip and shit
                let space_index = parsed_msg.find(' ').unwrap_or(parsed_msg.len());
                let cmd = &parsed_msg[0..space_index].to_lowercase();
                let args = &parsed_msg[space_index..].trim();

                match cmd.as_str() {
                    "!ping" => {
                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ChatPing)))?
                    }
                    "!sr" => {
                        if args.is_empty() {
                            let e = String::from("Correct usage: !sr <URL>");
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Invalid(e))))?;
                            continue;
                        }

                        let song = parsed_msg.splitn(2, ' ').collect::<Vec<&str>>()[1];
                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Sr((
                            parsed_sender,
                            song.to_string(),
                        )))))?;
                    }
                    "!skip" => {
                        if !tags.contains("mod=1") && parsed_sender.to_lowercase() != "sadmadladsalman" {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ModsOnly)))?;
                            continue;
                        }

                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::SkipSr)))?
                    }
                    "!queue" | "!q" => {
                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Queue)))?
                    }
                    "!currentsong" | "!song" => {
                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::CurrentSong)))?
                    }
                    "!currentspotify" | "!currentsp" => {
                        event_sender
                            .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::CurrentSongSpotify)))?;
                    }
                    "!volume" | "!v" => {
                        if args.is_empty() {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::GetVolume)))?;
                            continue;
                        }

                        if !tags.contains("mod=1") && parsed_sender.to_lowercase() != "sadmadladsalman" {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ModsOnly)))?;
                            continue;
                        }

                        let Ok(value) = args.parse::<i64>() else {
                            let e = String::from("Provide number");
                            event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Invalid(e))))?;
                            continue;
                        };

                        if !tags.contains("mod=1") && parsed_sender.to_lowercase() != "sadmadladsalman" {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ModsOnly)))?;
                            continue;
                        }

                        event_sender
                            .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::SetVolume(value))))?;
                    }
                    "!play" => event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Play)))?,
                    "!playspotify" | "!playsp" => {
                        if !tags.contains("mod=1") && parsed_sender.to_lowercase() != "sadmadladsalman" {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ModsOnly)))?;
                            continue;
                        }

                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::PlaySpotify)))?
                    }
                    "!stop" => {
                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Stop)))?
                    }
                    "!stopspotify" | "!stopsp" => {
                        if !tags.contains("mod=1") && parsed_sender.to_lowercase() != "sadmadladsalman" {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ModsOnly)))?;
                            continue;
                        }

                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::StopSpotify)))?
                    }
                    "!قوانين" => {
                        if !tags.contains("mod=1") && parsed_sender.to_lowercase() != "sadmadladsalman" {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ModsOnly)))?;
                            continue;
                        }

                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(
                            IrcChat::Rules,
                        )))?
                    },
                    "!title" => {
                        if args.is_empty() {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::GetTitle)))?;
                            continue;
                        }

                        if !tags.contains("mod=1") && parsed_sender.to_lowercase() != "sadmadladsalman" {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ModsOnly)))?;
                            continue;
                        }

                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::SetTitle(
                            args.to_string(),
                        ))))?;
                    }
                    "!warranty" => {
                        event_sender
                            .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Warranty)))?;
                        continue;
                    }
                    "!test" => {
                        if !tags.contains("mod=1") && parsed_sender.to_lowercase() != "sadmadladsalman" {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ModsOnly)))?;
                            continue;
                        }

                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Test(
                            args.to_string()
                        ))))?;
                    }
                    _ => {}
                }
            }
            Ok(Message::Text(msg)) => {
                if msg.contains("PING") {
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Ping)))?;
                } else if msg.contains("RECONNECT") {
                } else {
                    // println!("{msg}");
                }
            }
            Ok(_) => {}
            Err(e) => {
                return Err(eyre::eyre!("{e}"));
            }
        }
    }

    Ok(())
}

fn parse_message(msg: &str) -> String {
    let first = msg.trim()[(&msg[1..]).find(':').unwrap() + 2..].replace("\r\n", "");

    first[(&first[1..]).find(':').unwrap() + 2..].to_string()
}

fn parse_sender(msg: &str) -> String {
    let first = &msg.trim()[(&msg[1..]).find(':').unwrap() + 1..];
    
    first[1..first.find('!').unwrap()].to_string()
}

pub fn to_irc_message(msg: impl Into<String>) -> String {
    format!("PRIVMSG #sadmadladsalman :{}", msg.into())
}
