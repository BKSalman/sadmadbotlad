use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use crate::{
    db::Store,
    event_handler::{event_handler, Event, IrcChat, IrcEvent, IrcWs},
    flatten,
    song_requests::{play_song, setup_mpv, SongRequest},
    ApiInfo, APP,
};
use eyre::WrapErr;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use tokio::{
    net::TcpStream,
    sync::{
        mpsc::{UnboundedReceiver, UnboundedSender},
        RwLock,
    },
};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

pub async fn irc_connect(
    e_sender: UnboundedSender<Event>,
    e_receiver: UnboundedReceiver<Event>,
    api_info: Arc<ApiInfo>,
    store: Arc<Store>,
) -> eyre::Result<()> {
    println!("Starting IRC");

    let alerts_sender = APP.alerts_sender.clone();

    let sr_sender = APP.sr_sender.clone();

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
            alerts_sender,
            sr_sender,
            api_info,
            store,
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

    let event_senderc = event_sender.clone();

    let voters = Arc::new(RwLock::new(HashSet::new()));

    while let Some(msg) = locked_ws_receiver.next().await {
        match msg {
            Ok(Message::Ping(ping)) => {
                println!("IRC WebSocket Ping {ping:?}");
                event_sender.send(Event::IrcEvent(IrcEvent::WebSocket(IrcWs::Ping(ping))))?;
            }
            Ok(Message::Text(msg)) if msg.contains("PRIVMSG") => {
                let parsed_msg = parse_message(&msg);

                let tags = msg.rsplit_once(':').expect("no chat tags").0;
                let tags = parse_tags(tags);

                let parsed_sender = tags.sender();

                let mut command = String::new();
                let mut args = String::new();

                if tags.get_reply().is_ok() {
                    let first = &parsed_msg[parsed_msg.find(' ').unwrap_or(0) + 1..];
                    if !first.starts_with(APP.config.cmd_delim) {
                        continue;
                    }

                    let space_index = first.find(' ').unwrap_or(first.len());
                    command.push_str(&first[1..space_index].to_lowercase());
                    args.push_str(first[space_index..].trim());
                } else {
                    if !parsed_msg.starts_with(APP.config.cmd_delim) {
                        continue;
                    }

                    let space_index = parsed_msg.find(' ').unwrap_or(parsed_msg.len());
                    command.push_str(&parsed_msg[1..space_index].to_lowercase());
                    args.push_str(parsed_msg[space_index..].trim());
                }

                match command.as_str() {
                    "ping" => {
                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ChatPing)))?
                    }
                    "db" => {
                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Database)))?
                    }
                    "sr" => {
                        if args.is_empty() {
                            let e = format!("Correct usage: {}sr <URL>", APP.config.cmd_delim);
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
                    "skip" => {
                        if !tags.is_mod()? && parsed_sender.to_lowercase() != "sadmadladsalman" {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ModsOnly)))?;
                            continue;
                        }

                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::SkipSr)))?
                    }
                    "voteskip" => {
                        // TODO: figure out how to make every vote reset the timer (timeout??)

                        let votersc = voters.clone();

                        if voters.read().await.len() == 0 {
                            tokio::spawn(async move {
                                tokio::time::sleep(Duration::from_secs(20)).await;
                                votersc.write().await.clear();
                                println!("reset counter");
                            });
                        }

                        println!("{parsed_sender}");

                        voters.write().await.insert(parsed_sender);

                        println!("{}", voters.read().await.len());
                        event_senderc
                            .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::VoteSkip(
                                voters.read().await.len(),
                            ))))
                            .expect("skip");

                        if voters.read().await.len() >= 5 {
                            voters.write().await.clear();
                            event_senderc
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::SkipSr)))
                                .expect("skip");
                        }
                    }
                    "queue" | "q" => {
                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Queue)))?
                    }
                    "currentsong" | "song" => {
                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::CurrentSong)))?
                    }
                    "currentspotify" | "currentsp" => {
                        event_sender
                            .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::CurrentSongSpotify)))?;
                    }
                    "volume" | "v" => {
                        if args.is_empty() {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::GetVolume)))?;
                            continue;
                        }

                        if !tags.is_mod()? && parsed_sender.to_lowercase() != "sadmadladsalman" {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ModsOnly)))?;
                            continue;
                        }

                        let Ok(value) = args.parse::<i64>() else {
                            let e = String::from("Provide number");
                            event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Invalid(e))))?;
                            continue;
                        };

                        if !tags.is_mod()? && parsed_sender.to_lowercase() != "sadmadladsalman" {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ModsOnly)))?;
                            continue;
                        }

                        event_sender
                            .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::SetVolume(value))))?;
                    }
                    "play" => event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Play)))?,
                    "playspotify" | "playsp" => {
                        if !tags.is_mod()? && parsed_sender.to_lowercase() != "sadmadladsalman" {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ModsOnly)))?;
                            continue;
                        }

                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::PlaySpotify)))?
                    }
                    "stop" => event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Stop)))?,
                    "stopspotify" | "stopsp" => {
                        if !tags.is_mod()? && parsed_sender.to_lowercase() != "sadmadladsalman" {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ModsOnly)))?;
                            continue;
                        }

                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::StopSpotify)))?
                    }
                    "قوانين" => {
                        if !tags.is_mod()? && parsed_sender.to_lowercase() != "sadmadladsalman" {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ModsOnly)))?;
                            continue;
                        }

                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Rules)))?
                    }
                    "title" => {
                        if args.is_empty() {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::GetTitle)))?;
                            continue;
                        }

                        if !tags.is_mod()? && parsed_sender.to_lowercase() != "sadmadladsalman" {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ModsOnly)))?;
                            continue;
                        }

                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::SetTitle(
                            args.to_string(),
                        ))))?;
                    }
                    "warranty" => {
                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Warranty)))?;
                        continue;
                    }
                    "rustwarranty" | "!rwarranty" => {
                        event_sender
                            .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::RustWarranty)))?;
                        continue;
                    }
                    "workingon" | "wo" => {
                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::WorkingOn)))?;
                    }
                    "pixelperfect" | "pp" => {
                        event_sender
                            .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::PixelPerfect)))?;
                    }
                    "discord" | "disc" => {
                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Discord)))?;
                    }
                    "nerd" => {
                        if let Ok(reply) = tags.get_reply() {
                            let message = format!("Nerd \"{}\"", reply);
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Nerd(message))))?;
                        } else {
                            event_sender.send(Event::IrcEvent(IrcEvent::Chat(
                                IrcChat::Invalid(String::from(
                                    "Reply to a message to use this command",
                                )),
                            )))?;
                        }
                    }
                    "test" => {
                        if !tags.is_mod()? && parsed_sender.to_lowercase() != "sadmadladsalman" {
                            event_sender
                                .send(Event::IrcEvent(IrcEvent::Chat(IrcChat::ModsOnly)))?;
                            continue;
                        }

                        event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Test(
                            args.to_string(),
                        ))))?;
                    }
                    _ => {}
                }
            }
            Ok(Message::Text(msg)) => {
                #[allow(clippy::if_same_then_else)]
                if msg.contains("PING") {
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Ping)))?;
                } else if msg.contains("RECONNECT") {
                    // TODO: reconnect
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

#[derive(Debug)]
struct Tags(HashMap<String, String>);

impl Tags {
    fn is_mod(&self) -> eyre::Result<bool> {
        match self.0.get("mod") {
            Some(r#mod) => Ok(r#mod == "1"),
            None => Err(eyre::eyre!("No mod tag")),
        }
    }
    fn get_reply(&self) -> eyre::Result<String> {
        match self.0.get("reply-parent-msg-body") {
            Some(msg) => Ok(Self::decode_message(msg)),
            None => Err(eyre::eyre!("No reply tag")),
        }
    }
    fn decode_message(msg: &str) -> String {
        let mut output = String::with_capacity(msg.len());
        // taken from https://github.com/robotty/twitch-irc-rs/blob/2e3e36b646630a0e6059896d7fece413180fb253/src/message/tags.rs#L10
        let mut iter = msg.chars();
        while let Some(c) = iter.next() {
            if c == '\\' {
                let next_char = iter.next();
                match next_char {
                    Some(':') => output.push(';'),   // \: escapes to ;
                    Some('s') => output.push(' '),   // \s decodes to a space
                    Some('\\') => output.push('\\'), // \\ decodes to \
                    Some('r') => output.push('\r'),  // \r decodes to CR
                    Some('n') => output.push('\n'),  // \n decodes to LF
                    Some(c) => output.push(c),       // E.g. a\bc escapes to abc
                    None => {}                       // Dangling \ at the end of the string
                }
            } else {
                output.push(c);
            }
        }

        output
    }
    fn sender(&self) -> String {
        self.0
            .get("display-name")
            .expect("display name")
            .to_string()
    }
}

fn parse_tags(msg: &str) -> Tags {
    msg[1..]
        .split(';')
        .map(|s| {
            let (key, value) = s.split_once('=').expect("=");
            (key.to_string(), value.to_string())
        })
        .collect::<HashMap<String, String>>()
        .into()
}

impl From<HashMap<String, String>> for Tags {
    fn from(value: HashMap<String, String>) -> Self {
        Tags(value)
    }
}

fn parse_message(msg: &str) -> String {
    msg.rsplit_once(':').expect("message").1.trim().to_string()
}

pub fn to_irc_message(msg: impl Into<String>) -> String {
    format!("PRIVMSG #sadmadladsalman :{}", msg.into())
}
