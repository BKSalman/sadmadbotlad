use std::sync::Arc;

use crate::{
    event_handler::{self, event_handler, Event, IrcChat, IrcEvent, IrcWs},
    flatten,
    song_requests::{SongRequest, setup_mpv, play_song},
    ApiInfo,
};
use eyre::WrapErr;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use tokio::{net::TcpStream, sync::mpsc::UnboundedSender};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

pub async fn irc_connect() -> eyre::Result<()> {
    println!("Starting IRC");

    let (socket, _) = connect_async("wss://irc-ws.chat.twitch.tv:443").await?;

    let (ws_sender, ws_receiver) = socket.split();

    let ws_receiver = Arc::new(tokio::sync::Mutex::new(ws_receiver));

    let (e_sender, e_receiver) = tokio::sync::mpsc::unbounded_channel::<event_handler::Event>();

    let (song_sender, song_receiver) = tokio::sync::mpsc::channel::<SongRequest>(200);

    let e_sender_c = e_sender.clone();

    let mpv = Arc::new(setup_mpv());

    let mpv_c = mpv.clone();
    
    let join_handle =
        std::thread::spawn(move || play_song(mpv_c, song_receiver, e_sender_c));

    tokio::try_join!(
        flatten(tokio::spawn(read(e_sender, ws_receiver.clone()))),
        flatten(tokio::spawn(event_handler(
            song_sender,
            mpv,
            e_receiver,
            ws_sender,
            ws_receiver,
        ))),
    )
    .wrap_err_with(|| "songs")?;

    join_handle.join().expect("join")?;

    Ok(())
}

pub async fn irc_login<'a>(
    ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    api_info: &ApiInfo,
) -> Result<(), eyre::Report> {
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

                if parsed_msg.starts_with("!ping") {
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ChatPing)))?;
                } else if parsed_msg.starts_with("!sr ") {
                    let song = parsed_msg.splitn(2, ' ').collect::<Vec<&str>>()[1];
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Sr((
                        parsed_sender,
                        song.to_string(),
                    )))))?;
                } else if parsed_msg.starts_with("!sr") {
                    let e = String::from("Correct usage: !sr <URL>");
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Invalid(e))))?;
                } else if parsed_msg.starts_with("!skip") {
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::SkipSr)))?;
                } else if parsed_msg.starts_with("!queue") {
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Queue)))?;
                } else if parsed_msg.starts_with("!currentsong") {
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::CurrentSong)))?;
                } else if parsed_msg.starts_with("!volume ") {
                    let Ok(value) = parsed_msg.split(' ').collect::<Vec<&str>>()[1].parse::<i64>() else {
                        let e = String::from("Provide number");
                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Invalid(e))))?;
                        continue;
                    };
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::SetVolume(value))))?;
                } else if parsed_msg.starts_with("!volume") {
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::GetVolume)))?;
                } else if parsed_msg.starts_with("!pause") {
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::GetVolume)))?;
                } else if parsed_msg.starts_with("!play") {
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Play)))?;
                } else if parsed_msg.starts_with("!قوانين") {
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Rules)))?;
                }else if !parsed_msg.starts_with("!title ") && parsed_msg.starts_with("!title") {
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::GetTitle)))?;
                }
            }
            Ok(Message::Text(msg)) => {
                if msg.contains("PING") {
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Ping)))?;
                } else if msg.contains("RECONNECT") {
                    
                }
            }
            Ok(_) => {}
            Err(e) => {
                println!("{e}");
                // event_sender.send(Event::IrcEvent(IrcEvent::WebSocket(IrcWs::Unauthorized)))?;
                // this looks dangerous... I don't trust it
                // irc_connect().await?;

                return Err(eyre::ErrReport::new(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Unautherized")));
            },
        }
    }

    Ok(())
}

fn parse_message(msg: &str) -> String {
    msg.trim()[(&msg[1..]).find(':').unwrap() + 2..]
        .replace("\r\n", "")
        .to_string()
}

fn parse_sender(msg: &str) -> String {
    msg.trim()[1..msg.find('!').unwrap()].to_string()
}

pub fn to_irc_message(msg: impl Into<String>) -> String {
    format!("PRIVMSG #sadmadladsalman :{}", msg.into())
}
