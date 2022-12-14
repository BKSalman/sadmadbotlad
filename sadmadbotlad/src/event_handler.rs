use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{sink::SinkExt, StreamExt};
use libmpv::Mpv;
use std::process::Command;
use std::sync::Arc;
use tokio::{
    net::TcpStream,
    sync::mpsc::{Sender, UnboundedReceiver},
};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::db::Store;
use crate::song_requests::SrQueue;
use crate::twitch::{get_title, set_title};
use crate::{
    irc::{irc_login, to_irc_message},
    song_requests::SongRequest,
};
use crate::{Alert, AlertEventType, ApiInfo, SrFrontEndEvent, APP};

const RULES: &str = include_str!("../rules.txt");
const WARRANTY: &str = include_str!("../warranty.txt");
const RUST_WARRANTY: &str = include_str!("../rust_warranty.txt");

#[derive(Debug)]
pub enum Event {
    IrcEvent(IrcEvent),
    MpvEvent(MpvEvent),
}

#[derive(Debug)]
pub enum IrcEvent {
    WebSocket(IrcWs),
    Chat(IrcChat),
}

#[derive(Debug)]
pub enum IrcWs {
    Ping(Vec<u8>),
    Reconnect,
}

#[derive(Debug)]
pub enum IrcChat {
    Invalid(String),
    Ping,
    ChatPing,
    Sr((String, String)),
    SkipSr,
    Queue,
    CurrentSong,
    CurrentSongSpotify,
    SetVolume(i64),
    GetVolume,
    Stop,
    Play,
    Rules,
    GetTitle,
    SetTitle(String),
    ModsOnly,
    Warranty,
    RustWarranty,
    PlaySpotify,
    StopSpotify,
    Commercial,
    Database,
    Test(String),
}

#[derive(Debug)]
pub enum TestType {
    Raid,
    Follow,
}

#[derive(Debug)]
pub enum MpvEvent {
    DequeueSong,
    Error(i32),
}

#[allow(clippy::too_many_arguments)]
pub async fn event_handler(
    song_sender: Sender<SongRequest>,
    mpv: Arc<Mpv>,
    mut recv: UnboundedReceiver<Event>,
    mut ws_sender: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ws_receiver: Arc<tokio::sync::Mutex<SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>>>,
    front_end_alert_sender: tokio::sync::broadcast::Sender<Alert>,
    front_end_sr_sender: tokio::sync::broadcast::Sender<SrFrontEndEvent>,
    api_info: Arc<ApiInfo>,
    store: Arc<Store>,
) -> Result<(), eyre::Report> {
    let queue = Arc::new(tokio::sync::RwLock::new(SrQueue::new()));

    irc_login(&mut ws_sender, &api_info).await?;

    let sr_setupc = queue.clone();

    let t_handle = tokio::spawn(async move {
        front_end_receiver(front_end_sr_sender, sr_setupc).await;
    });

    while let Some(event) = recv.recv().await {
        match event {
            Event::IrcEvent(irc_event) => match irc_event {
                IrcEvent::WebSocket(event) => match event {
                    IrcWs::Ping(ping) => {
                        ws_sender.send(Message::Pong(ping)).await?;
                    }
                    IrcWs::Reconnect => {
                        println!("Reconnecting IRC");

                        let (socket, _) = connect_async("wss://irc-ws.chat.twitch.tv:443").await?;

                        let (sender, receiver) = socket.split();

                        ws_sender = sender;

                        let mut locked_ws_receiver = ws_receiver.lock().await;
                        *locked_ws_receiver = receiver;

                        irc_login(&mut ws_sender, &api_info).await?;
                    }
                },
                IrcEvent::Chat(event) => match event {
                    IrcChat::ChatPing => {
                        ws_sender
                            .send(Message::Text(to_irc_message(format!(
                                "{}pong",
                                APP.config.cmd_delim
                            ))))
                            .await?;
                    }
                    IrcChat::Ping => {
                        ws_sender
                            .send(Message::Text(String::from("PONG :tmi.twitch.tv")))
                            .await?;
                    }
                    IrcChat::Sr((sender, song)) => {
                        let message = queue
                            .write()
                            .await
                            .sr(&song, sender, song_sender.clone(), api_info.clone())
                            .await?;
                        ws_sender.send(Message::Text(message)).await?;
                    }
                    IrcChat::SkipSr => {
                        let mut message = String::new();
                        if let Some(song) = &queue.read().await.current_song {
                            if mpv.playlist_next_force().is_ok() {
                                message = to_irc_message(format!("Skipped: {}", song.title));
                            }
                        } else {
                            message = to_irc_message("No song playing");
                        }

                        ws_sender.send(Message::Text(message)).await?;
                    }
                    IrcChat::Invalid(e) => {
                        ws_sender.send(Message::Text(to_irc_message(e))).await?;
                    }
                    IrcChat::Queue => {
                        println!("{:#?}", queue.read().await.queue);
                        ws_sender
                            .send(Message::Text(to_irc_message(
                                "Check the queue at: https://f5rfm.bksalman.com",
                            )))
                            .await?;
                    }
                    IrcChat::CurrentSong => {
                        if let Some(current_song) = queue.read().await.current_song.as_ref() {
                            ws_sender
                                .send(Message::Text(to_irc_message(format!(
                                    "Current song: {} - by {}",
                                    current_song.title, current_song.user,
                                ))))
                                .await?;
                        } else {
                            ws_sender
                                .send(Message::Text(to_irc_message("No song playing")))
                                .await?;
                        }
                    }
                    IrcChat::CurrentSongSpotify => {
                        let cmd = Command::new("./scripts/current_spotify_song.sh").output()?;
                        let output = String::from_utf8(cmd.stdout)?;

                        ws_sender
                            .send(Message::Text(to_irc_message(format!(
                                "Current Spotify song: {}",
                                output
                            ))))
                            .await?;
                    }
                    IrcChat::PlaySpotify => {
                        if Command::new("./scripts/play_spotify.sh").spawn().is_ok() {
                            ws_sender
                                .send(Message::Text(to_irc_message(String::from(
                                    "Started playing spotify",
                                ))))
                                .await?;
                        } else {
                            println!("no script");
                        }
                    }
                    IrcChat::StopSpotify => {
                        if let Err(e) = Command::new("./scripts/pause_spotify.sh").spawn() {
                            println!("{e:?}");
                            continue;
                        }

                        ws_sender
                            .send(Message::Text(to_irc_message(String::from(
                                "Stopped playing spotify",
                            ))))
                            .await?;
                    }
                    IrcChat::SetVolume(volume) => {
                        if let Err(e) = mpv.set_property("volume", volume) {
                            println!("{e}");
                        }

                        ws_sender
                            .send(Message::Text(to_irc_message(format!(
                                "Volume set to {volume}",
                            ))))
                            .await?;
                    }
                    IrcChat::GetVolume => {
                        let Ok(volume) = mpv.get_property::<i64>("volume") else {
                            println!("volume error");
                            ws_sender.send(Message::Text(to_irc_message("No volume"))).await?;
                            continue;
                        };
                        ws_sender
                            .send(Message::Text(to_irc_message(format!("Volume: {}", volume))))
                            .await?;
                    }
                    IrcChat::Stop => {
                        if let Err(e) = mpv.pause() {
                            println!("{e}");
                        }

                        ws_sender
                            .send(Message::Text(to_irc_message(
                                "Stopped playing, !play to resume",
                            )))
                            .await?;
                    }
                    IrcChat::Play => {
                        if let Some(song) = &queue.read().await.current_song {
                            if let Err(e) = mpv.unpause() {
                                println!("{e}");
                            }

                            ws_sender
                                .send(Message::Text(to_irc_message(format!(
                                    "Resumed {}",
                                    song.title
                                ))))
                                .await?;
                        } else {
                            ws_sender
                                .send(Message::Text(to_irc_message("Queue is empty")))
                                .await?;
                        }
                    }
                    IrcChat::Rules => {
                        for rule in RULES.lines() {
                            ws_sender.send(Message::Text(to_irc_message(rule))).await?;
                        }
                    }
                    IrcChat::GetTitle => {
                        let title = get_title(&api_info).await?;
                        ws_sender
                            .send(Message::Text(to_irc_message(format!(
                                "Current stream title: {}",
                                title
                            ))))
                            .await?;
                    }
                    IrcChat::SetTitle(title) => {
                        set_title(&title, &api_info).await?;
                        ws_sender
                            .send(Message::Text(to_irc_message(format!(
                                "Set stream title to: {}",
                                title
                            ))))
                            .await?;
                    }
                    IrcChat::ModsOnly => {
                        ws_sender
                            .send(Message::Text(to_irc_message(String::from(
                                "This command can only be used by mods",
                            ))))
                            .await?;
                    }
                    IrcChat::Test(test) => match test.to_lowercase().as_str() {
                        "raid" => {
                            if let Err(e) = front_end_alert_sender.send(Alert {
                                new: true,
                                alert_type: AlertEventType::Raid {
                                    from: String::from("lmao"),
                                    viewers: 9999,
                                },
                            }) {
                                println!("frontend event failed:: {e:?}")
                            }
                        }
                        "follow" => {
                            if let Err(e) = front_end_alert_sender.send(Alert {
                                new: true,
                                alert_type: AlertEventType::Follow {
                                    follower: String::from("lmao"),
                                },
                            }) {
                                println!("frontend event failed:: {e:?}")
                            }
                        }
                        "sub" => {
                            if let Err(e) = front_end_alert_sender.send(Alert {
                                new: true,
                                alert_type: AlertEventType::Subscribe {
                                    subscriber: String::from("lmao"),
                                    tier: String::from("3"),
                                },
                            }) {
                                println!("frontend event failed:: {e:?}")
                            }
                        }
                        "resub" => {
                            if let Err(e) = front_end_alert_sender.send(Alert {
                                new: true,
                                alert_type: AlertEventType::ReSubscribe {
                                    subscriber: String::from("lmao"),
                                    tier: String::from("3"),
                                    subscribed_for: 4,
                                    streak: 2,
                                },
                            }) {
                                println!("frontend event failed:: {e:?}")
                            }
                        }
                        "giftsub" => {
                            if let Err(e) = front_end_alert_sender.send(Alert {
                                new: true,
                                alert_type: AlertEventType::GiftSub {
                                    gifter: String::from("lmao"),
                                    total: 9999,
                                    tier: String::from("3"),
                                },
                            }) {
                                println!("frontend event failed:: {e:?}")
                            }
                        }
                        _ => {
                            println!("no args provided");
                            if let Err(e) = front_end_alert_sender.send(Alert {
                                new: true,
                                alert_type: AlertEventType::Raid {
                                    from: String::from("lmao"),
                                    viewers: 9999,
                                },
                            }) {
                                println!("frontend event failed:: {e:?}")
                            }
                        }
                    },
                    IrcChat::Warranty => {
                        ws_sender
                            .send(Message::Text(to_irc_message(WARRANTY)))
                            .await?;
                    }
                    IrcChat::Commercial => {
                        ws_sender
                            .send(Message::Text(to_irc_message(
                                "Starting a 90 seconds commercial break",
                            )))
                            .await?;
                    }
                    IrcChat::RustWarranty => {
                        ws_sender
                            .send(Message::Text(to_irc_message(RUST_WARRANTY)))
                            .await?;
                    }
                    IrcChat::Database => {
                        let events = store.get_events().await?;

                        println!("{events:#?}");
                    }
                },
            },
            Event::MpvEvent(mpv_event) => match mpv_event {
                MpvEvent::DequeueSong => {
                    if queue.read().await.current_song.is_some() {
                        queue.write().await.current_song = None;
                        continue;
                    }

                    queue.write().await.dequeue();
                }
                MpvEvent::Error(e) => {
                    queue.write().await.current_song = None;
                    println!("MPV Error:: {e}");

                    // let e_str = e.to_string();

                    // let e = &e_str[e_str.find('(').unwrap() + 1..e_str.chars().count() - 1];

                    // println!(
                    //     "MpvEvent:: {e}:{}",
                    //     libmpv_sys::mpv_error_str(e.parse::<i32>().unwrap())
                    // );
                }
            },
        }
    }

    t_handle.abort();

    Ok(())
}

async fn front_end_receiver(
    front_end_events_sender: tokio::sync::broadcast::Sender<SrFrontEndEvent>,
    sr_setup: Arc<tokio::sync::RwLock<SrQueue>>,
) {
    let mut front_end_events_receiver = front_end_events_sender.subscribe();

    while let Ok(msg) = front_end_events_receiver.recv().await {
        match msg {
            SrFrontEndEvent::QueueRequest => {
                front_end_events_sender
                    .send(SrFrontEndEvent::QueueResponse(sr_setup.clone()))
                    .expect("song response");
            }
            SrFrontEndEvent::QueueResponse(_) => {}
        }
    }
}
