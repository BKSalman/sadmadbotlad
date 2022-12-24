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

use crate::twitch::{get_title, refresh_access_token, set_title};
use crate::FrontEndEvent;
use crate::{
    irc::{irc_login, to_irc_message},
    song_requests::{SongRequest, SongRequestSetup},
};

const RULES: &str = include_str!("../rules.txt");
const WARRANTY: &str = include_str!("../warranty.txt");

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
    Unauthorized,
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
    PlaySpotify,
    StopSpotify,
    Commercial,
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

pub async fn event_handler(
    song_sender: Sender<SongRequest>,
    mpv: Arc<Mpv>,
    mut recv: UnboundedReceiver<Event>,
    mut ws_sender: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ws_receiver: Arc<tokio::sync::Mutex<SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>>>,
    front_end_events_sender: tokio::sync::broadcast::Sender<FrontEndEvent>,
) -> Result<(), eyre::Report> {
    let sr_setup = Arc::new(tokio::sync::RwLock::new(SongRequestSetup::new().await?));
    let song_sender = Arc::new(song_sender);

    irc_login(&mut ws_sender, &sr_setup.read().await.api_info).await?;

    let fees = front_end_events_sender.clone();

    let sr_setupc = sr_setup.clone();

    let t_handle = tokio::spawn(async move {
        front_end_receiver(fees, sr_setupc).await;
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

                        irc_login(&mut ws_sender, &sr_setup.read().await.api_info).await?;
                    }
                    IrcWs::Unauthorized => {
                        refresh_access_token(&mut sr_setup.write().await.api_info).await?;
                    }
                },
                IrcEvent::Chat(event) => match event {
                    IrcChat::ChatPing => {
                        ws_sender
                            .send(Message::Text(to_irc_message("!pong")))
                            .await?;
                    }
                    IrcChat::Ping => {
                        ws_sender
                            .send(Message::Text(String::from("PONG :tmi.twitch.tv")))
                            .await?;
                    }
                    IrcChat::Sr((sender, song)) => {
                        let message = sr_setup
                            .write()
                            .await
                            .sr(&song, sender, song_sender.as_ref())
                            .await?;
                        ws_sender.send(Message::Text(message)).await?;
                    }
                    IrcChat::SkipSr => {
                        let mut message = String::new();
                        if let Some(song) = &sr_setup.read().await.queue.current_song {
                            if let Ok(_) = mpv.playlist_next_force() {
                                message = to_irc_message(format!("Skipped: {}", song.title));
                            }
                        } else {
                            message = to_irc_message("No song playing");
                        }

                        ws_sender.send(Message::Text(message)).await?;

                        // let e_str = e.to_string();

                        // let error = &e_str[e_str.find('(').unwrap() + 1..e_str.chars().count() - 1];

                        // std::io::Error::new(std::io::ErrorKind::Other, format!(
                        //     "MpvEvent:: {e}:{}",
                        //     libmpv_sys::mpv_error_str(error.parse::<i32>().unwrap())
                        // ))
                    }
                    IrcChat::Invalid(e) => {
                        ws_sender.send(Message::Text(to_irc_message(e))).await?;
                    }
                    IrcChat::Queue => {
                        println!("{:#?}", sr_setup.read().await.queue);
                        ws_sender
                            .send(Message::Text(to_irc_message(
                                "Check the queue at: https://f5rfm.bksalman.com",
                            )))
                            .await?;

                        // let mut message = String::new();
                        // for (s, i) in sr_setup.read().await.queue.queue.iter().zip(1..=20) {
                        //     if let Some(song) = s {
                        //         message
                        //             .push_str(&format!(" {i}- {} :: by {}", song.url, song.user,));
                        //     }
                        // }
                        // if message.chars().count() <= 0 {
                        //     ws_sender
                        //         .send(Message::Text(to_irc_message(
                        //             "No songs in queue, try !currentsong",
                        //         )))
                        //         .await?;
                        //     continue;
                        // }
                        // ws_sender
                        //     .send(Message::Text(to_irc_message(message)))
                        //     .await?;
                    }
                    IrcChat::CurrentSong => {
                        if let Some(current_song) =
                            sr_setup.read().await.queue.current_song.as_ref()
                        {
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
                        let cmd = Command::new("./current_spotify_song.sh").output()?;
                        let output = String::from_utf8(cmd.stdout)?;

                        ws_sender
                            .send(Message::Text(to_irc_message(format!(
                                "Current Spotify song: {}",
                                output
                            ))))
                            .await?;
                    }
                    IrcChat::PlaySpotify => {
                        if let Ok(_) = Command::new("./play_spotify.sh").spawn() {
                            ws_sender
                                .send(Message::Text(to_irc_message(String::from("Started playing spotify"))))
                                .await?;
                        } else {
                            println!("no script");
                        }
                    }
                    IrcChat::StopSpotify => {
                        if let Err(e) = Command::new("./pause_spotify.sh").spawn() {
                            println!("{e:?}");
                            continue;
                        }

                        ws_sender
                            .send(Message::Text(to_irc_message(String::from("Stopped playing spotify"))))
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
                            .send(Message::Text(to_irc_message("Paused")))
                            .await?;
                    }
                    IrcChat::Play => {
                        if let Some(song) = &sr_setup.read().await.queue.current_song {
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
                        let title = get_title(&mut sr_setup.write().await.api_info).await?;
                        ws_sender
                            .send(Message::Text(to_irc_message(format!(
                                "Current stream title: {}",
                                title
                            ))))
                            .await?;
                    }
                    IrcChat::SetTitle(title) => {
                        set_title(&title, &mut sr_setup.write().await.api_info).await?;
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
                            if let Err(e) = front_end_events_sender.send(FrontEndEvent::Raid {
                                from: String::from("lmao"),
                            }) {
                                println!("frontend event failed:: {e:?}")
                            }
                        }
                        "follow" => {
                            if let Err(e) = front_end_events_sender.send(FrontEndEvent::Follow {
                                follower: String::from("lmao"),
                            }) {
                                println!("frontend event failed:: {e:?}")
                            }
                        }
                        "sub" => {
                            if let Err(e) = front_end_events_sender.send(FrontEndEvent::Subscribe {
                                subscriber: String::from("lmao"),
                            }) {
                                println!("frontend event failed:: {e:?}")
                            }
                        }
                        _ => {
                            println!("no args provided");
                            if let Err(e) = front_end_events_sender.send(FrontEndEvent::Raid {
                                from: String::from("lmao"),
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
                            .send(Message::Text(to_irc_message("Starting a 90 seconds commercial break")))
                            .await?;
                    },
                },
            },
            Event::MpvEvent(mpv_event) => match mpv_event {
                MpvEvent::DequeueSong => {
                    if sr_setup.read().await.queue.current_song.is_some() {
                        sr_setup.write().await.queue.current_song = None;
                        continue;
                    }

                    sr_setup.write().await.queue.dequeue();
                }
                MpvEvent::Error(e) => {
                    sr_setup.write().await.queue.current_song = None;
                    println!("{e}");

                    let e_str = e.to_string();

                    let e = &e_str[e_str.find('(').unwrap() + 1..e_str.chars().count() - 1];

                    println!(
                        "MpvEvent:: {e}:{}",
                        libmpv_sys::mpv_error_str(e.parse::<i32>().unwrap())
                    );
                }
            },
        }
    }

    t_handle.abort();

    Ok(())
}

async fn front_end_receiver(
    front_end_events_sender: tokio::sync::broadcast::Sender<FrontEndEvent>,
    sr_setup: Arc<tokio::sync::RwLock<SongRequestSetup>>,
) {
    let mut front_end_events_receiver = front_end_events_sender.subscribe();

    while let Ok(msg) = front_end_events_receiver.recv().await {
        match msg {
            FrontEndEvent::Follow { follower: _ } => {}
            FrontEndEvent::Raid { from: _ } => {}
            FrontEndEvent::Subscribe { subscriber: _ } => {}
            FrontEndEvent::QueueRequest => {
                front_end_events_sender
                    .send(FrontEndEvent::SongsResponse(
                        sr_setup.read().await.queue.clone(),
                    ))
                    .expect("song response");
            }
            FrontEndEvent::SongsResponse(_) => {}
        }
    }
}
