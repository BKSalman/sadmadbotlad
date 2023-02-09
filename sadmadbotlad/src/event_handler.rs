use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{sink::SinkExt, StreamExt};
use libmpv::Mpv;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::{
    net::TcpStream,
    sync::mpsc::{Sender, UnboundedReceiver},
};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::commands::*;
use crate::db::Store;
use crate::song_requests::SrQueue;
use crate::{irc::irc_login, song_requests::SongRequest};
use crate::{Alert, ApiInfo, SrEvent};

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
    sr_sender: tokio::sync::broadcast::Sender<SrEvent>,
    api_info: Arc<ApiInfo>,
    store: Arc<Store>,
) -> Result<(), eyre::Report> {
    let queue = Arc::new(RwLock::new(SrQueue::new()));

    irc_login(&mut ws_sender, &api_info.twitch).await?;

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

                        irc_login(&mut ws_sender, &api_info.twitch).await?;
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
                        execute(InvalidCommand::new(e), &mut ws_sender).await?;
                    }
                    IrcChat::Queue => {
                        execute(QueueCommand::new(queue.clone()), &mut ws_sender).await?
                    }
                    IrcChat::CurrentSong => {
                        execute(CurrentSongCommand::new(queue.clone()), &mut ws_sender).await?
                    }
                    IrcChat::CurrentSongSpotify => {
                        execute(CurrentSongSpotifyCommand, &mut ws_sender).await?
                    }
                    IrcChat::PlaySpotify => {
                        execute(PlaySpotifyCommand, &mut ws_sender).await?;
                    }
                    IrcChat::StopSpotify => {
                        execute(StopSpotifyCommand, &mut ws_sender).await?;
                    }
                    IrcChat::SetVolume(volume) => {
                        execute(SetVolumeCommand::new(mpv.clone(), volume), &mut ws_sender).await?;
                    }
                    IrcChat::GetVolume => {
                        execute(GetVolumeCommand::new(mpv.clone()), &mut ws_sender).await?;
                    }
                    IrcChat::Stop => execute(StopCommand::new(mpv.clone()), &mut ws_sender).await?,
                    IrcChat::Play => {
                        execute(PlayCommand::new(queue.clone(), mpv.clone()), &mut ws_sender)
                            .await?;
                    }
                    IrcChat::Rules => {
                        execute(RulesCommand, &mut ws_sender).await?;
                    }
                    IrcChat::GetTitle => {
                        execute(GetTitleCommand::new(&api_info.twitch), &mut ws_sender).await?;
                    }
                    IrcChat::SetTitle(title) => {
                        execute(
                            SetTitleCommand::new(&api_info.twitch, title),
                            &mut ws_sender,
                        )
                        .await?;
                    }
                    IrcChat::ModsOnly => {
                        execute(ModsOnlyCommand, &mut ws_sender).await?;
                    }
                    IrcChat::Test(test) => {
                        execute(
                            AlertTestCommand::new(alert_sender.clone(), test),
                            &mut ws_sender,
                        )
                        .await?
                    }
                    IrcChat::Warranty => execute(WarrantyCommand, &mut ws_sender).await?,
                    IrcChat::Commercial => execute(CommercialBreakCommand, &mut ws_sender).await?,
                    IrcChat::RustWarranty => execute(RustWarrantyCommand, &mut ws_sender).await?,
                    IrcChat::Database => {
                        execute(DatabaseCommand::new(store.clone()), &mut ws_sender).await?
                    }
                    IrcChat::WorkingOn => execute(WorkingOnCommand, &mut ws_sender).await?,
                    IrcChat::PixelPerfect => execute(PixelPerfectCommand, &mut ws_sender).await?,
                    IrcChat::Discord => execute(DiscordCommand, &mut ws_sender).await?,
                    IrcChat::Nerd(message) => {
                        execute(NerdCommand::new(message), &mut ws_sender).await?;
                    }
                    IrcChat::VoteSkip(voters) => {
                        execute(VoteSkipCommand::new(voters), &mut ws_sender).await?;
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
    front_end_events_sender: tokio::sync::broadcast::Sender<SrEvent>,
    sr_setup: Arc<tokio::sync::RwLock<SrQueue>>,
) {
    let mut events_receiver = front_end_events_sender.subscribe();

    while let Ok(msg) = events_receiver.recv().await {
        match msg {
            SrEvent::QueueRequest => {
                front_end_events_sender
                    .send(SrEvent::QueueResponse(sr_setup.clone()))
                    .expect("song response");
            }
            SrEvent::QueueResponse(_) => {}
        }
    }
}
