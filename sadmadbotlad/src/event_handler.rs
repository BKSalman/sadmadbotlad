use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{sink::SinkExt, StreamExt};
use libmpv::Mpv;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::{
    net::TcpStream,
    sync::mpsc::{Sender, UnboundedReceiver},
};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::commands::current_song::CurrentSongCommand;
use crate::commands::execute;
use crate::commands::ping::PingCommand;
use crate::commands::queue::QueueCommand;
use crate::commands::seventv::SevenTvCommand;
use crate::commands::skip_sr::SkipSrCommand;
use crate::commands::sr::SrCommand;
use crate::db::Store;
use crate::song_requests::SrQueue;
use crate::twitch::{get_title, set_title};
use crate::{
    irc::{irc_login, to_irc_message},
    song_requests::SongRequest,
};
use crate::{Alert, AlertEventType, ApiInfo, SrFrontEndEvent};

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
    ChatPing(String),
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
    WorkingOn,
    PixelPerfect,
    Discord,
    Nerd(String),
    VoteSkip(usize),
    SevenTv(String),
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
    alert_sender: tokio::sync::broadcast::Sender<Alert>,
    sr_sender: tokio::sync::broadcast::Sender<SrFrontEndEvent>,
    api_info: Arc<RwLock<ApiInfo>>,
    store: Arc<Store>,
) -> Result<(), eyre::Report> {
    let queue = Arc::new(RwLock::new(SrQueue::new()));

    irc_login(&mut ws_sender, &*api_info.read().await).await?;

    let queuec = queue.clone();

    let t_handle = tokio::spawn(async move {
        front_end_receiver(sr_sender, queuec).await;
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

                        irc_login(&mut ws_sender, &*api_info.read().await).await?;
                    }
                },
                IrcEvent::Chat(event) => match event {
                    IrcChat::Ping => {
                        ws_sender
                            .send(Message::Text(String::from("PONG :tmi.twitch.tv\r\n")))
                            .await?;
                    }
                    IrcChat::ChatPing(command) => {
                        execute(PingCommand::new(command), &mut ws_sender).await?
                    }
                    IrcChat::Sr((sender, song)) => {
                        execute(
                            SrCommand::new(
                                queue.clone(),
                                song,
                                sender,
                                song_sender.clone(),
                                api_info.clone(),
                            ),
                            &mut ws_sender,
                        )
                        .await?;
                    }
                    IrcChat::SkipSr => {
                        execute(
                            SkipSrCommand::new(queue.clone(), mpv.clone()),
                            &mut ws_sender,
                        )
                        .await?
                    }
                    IrcChat::Invalid(e) => {
                        ws_sender.send(Message::Text(to_irc_message(e))).await?;
                    }
                    IrcChat::Queue => {
                        execute(QueueCommand::new(queue.clone()), &mut ws_sender).await?
                    }
                    IrcChat::CurrentSong => {
                        execute(CurrentSongCommand::new(queue.clone()), &mut ws_sender).await?
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
                        let title = get_title(api_info.clone()).await?;
                        ws_sender
                            .send(Message::Text(to_irc_message(format!(
                                "Current stream title: {}",
                                title
                            ))))
                            .await?;
                    }
                    IrcChat::SetTitle(title) => {
                        set_title(&title, api_info.clone()).await?;
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
                            if let Err(e) = alert_sender.send(Alert {
                                new: true,
                                r#type: AlertEventType::Raid {
                                    from: String::from("lmao"),
                                    viewers: 9999,
                                },
                            }) {
                                println!("frontend event failed:: {e:?}")
                            }
                        }
                        "follow" => {
                            if let Err(e) = alert_sender.send(Alert {
                                new: true,
                                r#type: AlertEventType::Follow {
                                    follower: String::from("lmao"),
                                },
                            }) {
                                println!("frontend event failed:: {e:?}")
                            }
                        }
                        "sub" => {
                            if let Err(e) = alert_sender.send(Alert {
                                new: true,
                                r#type: AlertEventType::Subscribe {
                                    subscriber: String::from("lmao"),
                                    tier: String::from("3"),
                                },
                            }) {
                                println!("frontend event failed:: {e:?}")
                            }
                        }
                        "resub" => {
                            if let Err(e) = alert_sender.send(Alert {
                                new: true,
                                r#type: AlertEventType::ReSubscribe {
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
                            if let Err(e) = alert_sender.send(Alert {
                                new: true,
                                r#type: AlertEventType::GiftSub {
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
                            if let Err(e) = alert_sender.send(Alert {
                                new: true,
                                r#type: AlertEventType::Raid {
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
                    IrcChat::WorkingOn => {
                        ws_sender
                            .send(Message::Text(to_irc_message("You can check what I'm working in here: https://gist.github.com/BKSalman/090658c8f67cc94bfb9d582d5be68ed4")))
                            .await?;
                    }
                    IrcChat::PixelPerfect => {
                        ws_sender
                            .send(Message::Text(to_irc_message("image_res.rect")))
                            .await?;
                    }
                    IrcChat::Discord => {
                        ws_sender
                            .send(Message::Text(to_irc_message("https://discord.gg/qs4SGUt")))
                            .await?;
                    }
                    IrcChat::Nerd(message) => {
                        ws_sender
                            .send(Message::Text(to_irc_message(message)))
                            .await?;
                    }
                    IrcChat::VoteSkip(voters) => {
                        let message = format!("{voters}/5 skip votes");

                        ws_sender
                            .send(Message::Text(to_irc_message(message)))
                            .await?;
                    }
                    IrcChat::SevenTv(query) => {
                        execute(SevenTvCommand::new(query), &mut ws_sender).await?
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
    let mut events_receiver = front_end_events_sender.subscribe();

    while let Ok(msg) = events_receiver.recv().await {
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
