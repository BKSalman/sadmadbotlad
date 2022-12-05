use std::sync::Arc;

use futures_util::sink::SinkExt;
use futures_util::stream::SplitSink;
use tokio::{
    net::TcpStream,
    sync::mpsc::{Sender, UnboundedReceiver},
};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::{
    irc::irc_login,
    song_requests::{SongRequest, SongRequestSetup},
};

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
}

#[derive(Debug)]
pub enum IrcChat {
    Ping,
    Sr((String, String)),
    SkipSr,
    Invalid(String),
}

#[derive(Debug)]
pub enum MpvEvent {
    DequeueSong,
    Error(i32),
}

pub async fn event_handler(
    song_sender: Sender<SongRequest>,
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
                            .send(Message::Text(String::from(
                                "PRIVMSG #sadmadladsalman :!pong",
                            )))
                            .await?;
                    }
                },
                IrcEvent::Chat(event) => match event {
                    IrcChat::Ping => {
                        ws_sender
                            .send(Message::Text(String::from(
                                "PRIVMSG #sadmadladsalman :!pong",
                            )))
                            .await?;
                    }
                    IrcChat::Sr((sender, song)) => {
                        let message = sr_setup.sr(&song, sender, song_sender.as_ref()).await?;
                        ws_sender.send(Message::Text(message)).await?;
                    }
                    IrcChat::SkipSr => {
                        let message = sr_setup.skip()?;
                        ws_sender.send(Message::Text(message)).await?;
                    }
                    IrcChat::Invalid(e) => {
                        ws_sender.send(Message::Text(e)).await?;
                    }
                },
            },
            Event::MpvEvent(mpv_event) => match mpv_event {
                MpvEvent::DequeueSong => {
                    let Ok(Some(song)) = sr_setup.queue.dequeue() else {
                        println!("nothing");
                        continue;
                    };

                    println!("{song:?}");
                }
                MpvEvent::Error(e) => {
                    sr_setup.queue.current_song = None;
                    println!("{e}");

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

    Ok(())
}
