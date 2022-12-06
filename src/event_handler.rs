use futures_util::{sink::SinkExt, StreamExt};
use futures_util::stream::SplitSink;
use libmpv::Mpv;
use std::sync::Arc;
use std::time::Duration;
use tokio::{
    net::TcpStream,
    sync::mpsc::{Sender, UnboundedReceiver},
};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream, connect_async};

use crate::{
    irc::{irc_login, to_irc_message},
    song_requests::{SongRequest, SongRequestSetup},
};

const RULES: &str = include_str!("../rules.txt");

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
    Ping,
    Reconnect,
}

#[derive(Debug)]
pub enum IrcChat {
    Invalid(String),
    Ping,
    Sr((String, String)),
    SkipSr,
    Queue,
    CurrentSong,
    SetVolume(i64),
    GetVolume,
    Pause,
    Play,
    Rules,
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
) -> Result<(), eyre::Report> {
    let mut sr_setup = SongRequestSetup::new();
    let song_sender = Arc::new(song_sender);

    irc_login(&mut ws_sender, &sr_setup.api_info).await?;

    while let Some(event) = recv.recv().await {
        match event {
            Event::IrcEvent(irc_event) => match irc_event {
                IrcEvent::WebSocket(event) => match event {
                    IrcWs::Ping => {
                        ws_sender
                            .send(Message::Text(String::from("PONG :tmi.twitch.tv")))
                            .await?;
                    }
                    IrcWs::Reconnect => {
                        println!("Reconnecting IRC");

                        let (socket, _) = connect_async("wss://irc-ws.chat.twitch.tv:443").await?;

                        let (sender, _recv) = socket.split();
                        
                        ws_sender = sender;

                        // TODO: do this later lazy ass bitch
                        // ws_receiver = recv;

                        irc_login(&mut ws_sender, &sr_setup.api_info).await?;
                    }
                },
                IrcEvent::Chat(event) => match event {
                    IrcChat::Ping => {
                        ws_sender
                            .send(Message::Text(to_irc_message("!pong")))
                            .await?;
                    }
                    IrcChat::Sr((sender, song)) => {
                        let message = sr_setup.sr(&song, sender, song_sender.as_ref()).await?;
                        ws_sender.send(Message::Text(message)).await?;
                    }
                    IrcChat::SkipSr => {
                        if let Some(song) = &sr_setup.queue.current_song {
                            if let Ok(_) = mpv.playlist_next_force() {
                                to_irc_message(format!("Skipped {}", song.title));
                            }
                        } else {
                            ws_sender
                                .send(Message::Text(to_irc_message("No song playing")))
                                .await?;
                        }
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
                        println!("{:#?}", sr_setup.queue);
                        let mut message = String::new();
                        for (s, i) in sr_setup.queue.queue.iter().zip(1..=20) {
                            if let Some(song) = s {
                                message.push_str(&format!(" {i}- {} :: by {}", song.url, song.user,));
                            }
                        }
                        ws_sender.send(Message::Text(to_irc_message(message))).await?;
                    }
                    IrcChat::CurrentSong => {
                        if let Some(current_song) = sr_setup.queue.current_song.as_ref() {
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
                    IrcChat::Pause => {
                        if let Err(e) = mpv.pause() {
                            println!("{e}");
                        }

                        ws_sender
                            .send(Message::Text(to_irc_message("Paused")))
                            .await?;
                    }
                    IrcChat::Play => {
                        if let Some(song) = &sr_setup.queue.current_song {
                            if let Err(e) = mpv.unpause() {
                                println!("{e}");
                            }

                            ws_sender
                                .send(Message::Text(to_irc_message(format!("Resumed {:?}", song))))
                                .await?;
                        } else {
                            ws_sender
                                .send(Message::Text(to_irc_message("Queue is empty")))
                                .await?;
                        }
                    }
                    IrcChat::Rules => {
                        for rule in RULES.lines() {
                            ws_sender
                                .send(Message::Text(to_irc_message(rule)))
                                .await?;
                            std::thread::sleep(Duration::from_millis(500));
                        }
                    },
                },
            },
            Event::MpvEvent(mpv_event) => match mpv_event {
                MpvEvent::DequeueSong => {
                    if let Some(song) = &sr_setup.queue.current_song {
                        println!("{song:#?}");
                    }
                    sr_setup.queue.dequeue();
                }
                MpvEvent::Error(e) => {
                    sr_setup.queue.current_song = None;
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

    Ok(())
}
