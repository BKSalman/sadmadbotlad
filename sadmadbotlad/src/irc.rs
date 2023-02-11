use std::{
    collections::{HashMap, HashSet},
    process,
    sync::Arc,
    time::Duration,
};

use crate::{
    db::Store,
    // event_handler::{Event, IrcChat, IrcEvent, IrcWs},
    flatten,
    song_requests::{play_song, setup_mpv, QueueMessages, SongRequest},
    twitch::{get_title, set_title, TwitchTokenMessages},
    Alert,
    AlertEventType,
    APP,
};
use eyre::Context;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use libmpv::Mpv;
use tokio::{
    net::TcpStream,
    sync::{
        broadcast,
        mpsc::{self, UnboundedSender},
        oneshot, RwLock,
    },
    task::JoinHandle,
};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

const RUST_WARRANTY: &str = include_str!("../rust_warranty.txt");

const WARRANTY: &str = include_str!("../warranty.txt");

const RULES: &str = include_str!("../rules.txt");

pub async fn irc_connect(
    alerts_sender: broadcast::Sender<Alert>,
    song_receiver: mpsc::Receiver<SongRequest>,
    queue_sender: mpsc::UnboundedSender<QueueMessages>,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    store: Arc<Store>,
) -> eyre::Result<()> {
    println!("Starting IRC");

    let (socket, _) = connect_async("wss://irc-ws.chat.twitch.tv:443").await?;

    let (ws_sender, ws_receiver) = socket.split();

    let ws_receiver = Arc::new(tokio::sync::Mutex::new(ws_receiver));

    // let e_sender_c = e_sender.clone();

    let mpv = Arc::new(setup_mpv());

    let mpv_c = mpv.clone();

    let t_handle = {
        let queue_sender = queue_sender.clone();
        std::thread::spawn(move || play_song(mpv_c, song_receiver, queue_sender))
    };

    tokio::try_join!(
        flatten(tokio::spawn(read(
            // e_sender,
            ws_receiver.clone(),
            ws_sender,
            queue_sender,
            token_sender,
            mpv,
            alerts_sender,
            store,
        ))),
        // flatten(tokio::spawn(event_handler(
        //     mpv,
        //     e_receiver,
        //     ws_receiver,
        //     alerts_sender,
        //     sr_sender,
        //     api_info,
        //     token_sender,
        //     store,
        // ))),
    )
    .wrap_err_with(|| "something")?;

    t_handle.join().expect("play_song thread")?;

    Ok(())
}

pub async fn irc_login(
    ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    token_sender: UnboundedSender<TwitchTokenMessages>,
) -> Result<(), eyre::Report> {
    let cap = Message::Text(String::from("CAP REQ :twitch.tv/commands twitch.tv/tags"));

    ws_sender.send(cap).await?;

    let (one_shot_sender, one_shot_receiver) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(one_shot_sender))?;

    let Ok(api_info) = one_shot_receiver.await else {
        return Err(eyre::eyre!("Failed to get token"));
    };

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
    // event_sender: UnboundedSender<Event>,
    ws_receiver: Arc<tokio::sync::Mutex<SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>>>,
    mut ws_sender: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    queue_sender: mpsc::UnboundedSender<QueueMessages>,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    mpv: Arc<Mpv>,
    alerts_sender: broadcast::Sender<Alert>,
    store: Arc<Store>,
) -> eyre::Result<()> {
    let mut locked_ws_receiver = ws_receiver.lock().await;

    // let event_senderc = event_sender.clone();

    let voters = Arc::new(RwLock::new(HashSet::new()));

    irc_login(&mut ws_sender, token_sender.clone()).await?;

    while let Some(msg) = locked_ws_receiver.next().await {
        match msg {
            Ok(Message::Ping(ping)) => {
                println!("IRC WebSocket Ping {ping:?}");
                ws_sender.send(Message::Pong(vec![])).await?;
            }
            Ok(Message::Text(msg)) if msg.contains("PRIVMSG") => {
                let parsed_msg = parse_irc(&msg);

                let (command, args) = if parsed_msg.tags.get_reply().is_ok() {
                    // remove mention
                    let (_, message) = parsed_msg.message.split_once(' ').expect("remove mention");

                    if !message.starts_with(APP.config.cmd_delim) {
                        continue;
                    }

                    message.split_once(' ').unwrap_or((message, ""))
                } else {
                    if !parsed_msg.message.starts_with(APP.config.cmd_delim) {
                        continue;
                    }

                    parsed_msg
                        .message
                        .split_once(' ')
                        .unwrap_or((&parsed_msg.message, ""))
                };

                match &command.to_lowercase()[1..] {
                    "ping" | "وكز" => {
                        if command.is_ascii() {
                            ws_sender
                                .send(Message::Text(to_irc_message(&format!(
                                    "{}pong",
                                    APP.config.cmd_delim
                                ))))
                                .await?;
                        } else {
                            ws_sender
                                .send(Message::Text(to_irc_message(&format!(
                                    "{}يكز",
                                    APP.config.cmd_delim
                                ))))
                                .await?;
                        }
                    }
                    "db" => {
                        let events = store.get_events().await?;

                        println!("{events:#?}");
                    }
                    "sr" => {
                        if args.is_empty() {
                            let e = format!("Correct usage: {}sr <URL>", APP.config.cmd_delim);
                            ws_sender.send(Message::Text(to_irc_message(&e))).await?;
                            continue;
                        }

                        let (send, recv) = oneshot::channel();

                        queue_sender.send(QueueMessages::Sr(
                            (parsed_msg.sender, args.to_string()),
                            send,
                        ))?;

                        let Ok(message) = recv.await else {
                            return Err(eyre::eyre!("Could not request song"));
                        };

                        ws_sender
                            .send(Message::Text(to_irc_message(&message)))
                            .await?;
                    }
                    "skip" => {
                        if let Some(message) = mods_only(&parsed_msg)? {
                            ws_sender
                                .send(Message::Text(to_irc_message(&message)))
                                .await?;
                            continue;
                        }

                        let (send, recv) = oneshot::channel();

                        queue_sender.send(QueueMessages::GetCurrentSong(send))?;

                        let Ok(current_song) = recv.await else {
                            return Err(eyre::eyre!("Could not get current song"));
                        };

                        let mut message = String::new();

                        if let Some(song) = current_song {
                            queue_sender.send(QueueMessages::Dequeue)?;

                            if mpv.playlist_next_force().is_ok() {
                                message = format!("Skipped: {}", song.title);
                            }
                        } else {
                            message = String::from("No song playing");
                        }

                        ws_sender
                            .send(Message::Text(to_irc_message(&message)))
                            .await?;
                    }
                    "voteskip" => {
                        // TODO: figure out how to make every vote reset the timer (timeout??)

                        let votersc = voters.clone();

                        let mut t_handle: Option<JoinHandle<()>> = None;

                        if voters.read().await.len() == 0 {
                            t_handle = Some(tokio::spawn(async move {
                                tokio::time::sleep(Duration::from_secs(20)).await;
                                votersc.write().await.clear();
                                println!("reset counter");
                            }));
                        }

                        voters.write().await.insert(parsed_msg.sender);

                        println!("{}", voters.read().await.len());

                        ws_sender
                            .send(Message::Text(to_irc_message(&format!(
                                "{}/4 skip votes",
                                voters.read().await.len()
                            ))))
                            .await?;

                        if voters.read().await.len() >= 4 {
                            voters.write().await.clear();

                            let (send, recv) = oneshot::channel();

                            queue_sender.send(QueueMessages::GetCurrentSong(send))?;

                            let Ok(current_song) = recv.await else {
                                return Err(eyre::eyre!("Could not get current song"));
                            };

                            let message = if let Some(song) = current_song {
                                queue_sender.send(QueueMessages::Dequeue)?;

                                if let Some(t_handle) = t_handle {
                                    t_handle.abort();
                                }

                                if mpv.playlist_next_force().is_ok() {
                                    format!("Skipped: {}", song.title)
                                } else {
                                    String::from("lmao")
                                }
                            } else {
                                String::from("No song playing")
                            };

                            ws_sender
                                .send(Message::Text(to_irc_message(&message)))
                                .await?;
                        }
                    }
                    "queue" | "q" => {
                        ws_sender
                            .send(Message::Text(to_irc_message(
                                "Check the queue at: https://f5rfm.bksalman.com",
                            )))
                            .await?;
                    }
                    "currentsong" | "song" => {
                        let (send, recv) = oneshot::channel();

                        queue_sender.send(QueueMessages::GetCurrentSong(send))?;

                        let Ok(current_song) = recv.await else {
                            return Err(eyre::eyre!("Could not get current song"));
                        };

                        let message = if let Some(song) = current_song {
                            format!(
                                "current song: {} , requested by {} - {}",
                                song.title, song.user, song.url,
                            )
                        } else {
                            String::from("No song playing")
                        };

                        ws_sender
                            .send(Message::Text(to_irc_message(&message)))
                            .await?;
                    }
                    "currentspotify" | "currentsp" => {
                        let cmd =
                            process::Command::new("./scripts/current_spotify_song.sh").output()?;
                        let output = String::from_utf8(cmd.stdout)?;

                        ws_sender
                            .send(Message::Text(to_irc_message(&format!(
                                "Current Spotify song: {}",
                                output
                            ))))
                            .await?;
                    }
                    "volume" | "v" => {
                        if args.is_empty() {
                            let Ok(volume) = mpv.get_property::<i64>("volume") else {
                                                println!("volume error");
                                                ws_sender.send(Message::Text(to_irc_message("No volume"))).await?;
                                                return Ok(());
                                            };
                            ws_sender
                                .send(Message::Text(to_irc_message(&format!(
                                    "Volume: {}",
                                    volume
                                ))))
                                .await?;
                            continue;
                        }

                        if let Some(message) = mods_only(&parsed_msg)? {
                            ws_sender.send(Message::Text(message)).await?;
                            continue;
                        }

                        let Ok(volume) = args.parse::<i64>() else {
                            let e = String::from("Provide number");
                            ws_sender.send(Message::Text(e)).await?;
                            continue;
                        };

                        if let Err(e) = mpv.set_property("volume", volume) {
                            println!("{e}");
                        }

                        ws_sender
                            .send(Message::Text(to_irc_message(&format!(
                                "Volume set to {}",
                                volume
                            ))))
                            .await?;
                    }
                    "play" => {
                        if let Some(message) = mods_only(&parsed_msg)? {
                            ws_sender
                                .send(Message::Text(to_irc_message(&message)))
                                .await?;
                            continue;
                        }

                        let (send, recv) = oneshot::channel();

                        queue_sender.send(QueueMessages::GetCurrentSong(send))?;

                        let Ok(current_song) = recv.await else {
                            return Err(eyre::eyre!("Could not get current song"));
                        };

                        if let Some(song) = current_song {
                            if let Err(e) = mpv.unpause() {
                                println!("{e}");
                            }

                            ws_sender
                                .send(Message::Text(to_irc_message(&format!(
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
                    "playspotify" | "playsp" => {
                        if let Some(message) = mods_only(&parsed_msg)? {
                            ws_sender
                                .send(Message::Text(to_irc_message(&message)))
                                .await?;
                            continue;
                        }

                        if process::Command::new("./scripts/play_spotify.sh")
                            .spawn()
                            .is_ok()
                        {
                            ws_sender
                                .send(Message::Text(to_irc_message("Started playing spotify")))
                                .await?;
                        } else {
                            println!("no script");
                        }
                    }
                    "stop" => {
                        if let Some(message) = mods_only(&parsed_msg)? {
                            ws_sender
                                .send(Message::Text(to_irc_message(&message)))
                                .await?;
                            continue;
                        }

                        let (send, recv) = oneshot::channel();

                        queue_sender.send(QueueMessages::GetCurrentSong(send))?;

                        let Ok(current_song) = recv.await else {
                            return Err(eyre::eyre!("Could not get current song"));
                        };

                        if let Some(song) = current_song {
                            if let Err(e) = mpv.pause() {
                                println!("{e}");
                                continue;
                            }

                            ws_sender
                                .send(Message::Text(to_irc_message(&format!(
                                    "Stopped playing {}, !play to resume",
                                    song.title
                                ))))
                                .await?;
                        } else {
                            ws_sender
                                .send(Message::Text(to_irc_message("Queue is empty")))
                                .await?;
                        }
                    }
                    "stopspotify" | "stopsp" => {
                        if let Some(message) = mods_only(&parsed_msg)? {
                            ws_sender
                                .send(Message::Text(to_irc_message(&message)))
                                .await?;
                            continue;
                        }

                        if let Err(e) = process::Command::new("./scripts/pause_spotify.sh").spawn()
                        {
                            println!("{e}");
                            continue;
                        }

                        ws_sender
                            .send(Message::Text(to_irc_message("Stopped playing spotify")))
                            .await?;
                    }
                    "قوانين" => {
                        if let Some(message) = mods_only(&parsed_msg)? {
                            ws_sender
                                .send(Message::Text(to_irc_message(&message)))
                                .await?;
                            continue;
                        }

                        for rule in RULES.lines() {
                            ws_sender.send(Message::Text(to_irc_message(rule))).await?;
                        }
                    }
                    "title" => {
                        if args.is_empty() {
                            let title = get_title(token_sender.clone()).await?;

                            ws_sender
                                .send(Message::Text(to_irc_message(&format!(
                                    "Current stream title: {}",
                                    title
                                ))))
                                .await?;

                            continue;
                        }

                        if let Some(message) = mods_only(&parsed_msg)? {
                            ws_sender
                                .send(Message::Text(to_irc_message(&message)))
                                .await?;
                            continue;
                        }

                        set_title(args, token_sender.clone()).await?;
                        ws_sender
                            .send(Message::Text(to_irc_message(&format!(
                                "Set stream title to: {}",
                                args
                            ))))
                            .await?;
                    }
                    "warranty" => {
                        ws_sender
                            .send(Message::Text(to_irc_message(WARRANTY)))
                            .await?;
                        continue;
                    }
                    "rustwarranty" | "!rwarranty" => {
                        ws_sender
                            .send(Message::Text(to_irc_message(RUST_WARRANTY)))
                            .await?;
                        continue;
                    }
                    "workingon" | "wo" => {
                        ws_sender
                            .send(Message::Text(to_irc_message(
                                "You can check what I'm working in here: \
                                https://gist.github.com/BKSalman/090658c8f67cc94bfb9d582d5be68ed4",
                            )))
                            .await?;
                    }
                    "pixelperfect" | "pp" => {
                        ws_sender
                            .send(Message::Text(to_irc_message("image_res.rect")))
                            .await?;
                    }
                    "discord" | "disc" => {
                        ws_sender
                            .send(Message::Text(to_irc_message("https://discord.gg/qs4SGUt")))
                            .await?;
                    }
                    "nerd" => {
                        if let Ok(reply) = parsed_msg.tags.get_reply() {
                            let message = format!("Nerd \"{}\"", reply);
                            ws_sender
                                .send(Message::Text(to_irc_message(&message)))
                                .await?;
                        } else {
                            ws_sender
                                .send(Message::Text(String::from(
                                    "Reply to a message to use this command",
                                )))
                                .await?;
                        }
                    }
                    "7tv" => {
                        ws_sender
                            .send(Message::Text(to_irc_message(&format!(
                                "https://7tv.app/emotes?query={args}"
                            ))))
                            .await?;
                    }
                    "test" => {
                        if let Some(message) = mods_only(&parsed_msg)? {
                            ws_sender
                                .send(Message::Text(to_irc_message(&message)))
                                .await?;
                            continue;
                        }

                        let alert = match args.to_lowercase().as_str() {
                            "raid" => Alert {
                                new: true,
                                r#type: AlertEventType::Raid {
                                    from: String::from("lmao"),
                                    viewers: 9999,
                                },
                            },
                            "follow" => Alert {
                                new: true,
                                r#type: AlertEventType::Follow {
                                    follower: String::from("lmao"),
                                },
                            },
                            "sub" => Alert {
                                new: true,
                                r#type: AlertEventType::Subscribe {
                                    subscriber: String::from("lmao"),
                                    tier: String::from("3"),
                                },
                            },
                            "resub" => Alert {
                                new: true,
                                r#type: AlertEventType::ReSubscribe {
                                    subscriber: String::from("lmao"),
                                    tier: String::from("3"),
                                    subscribed_for: 4,
                                    streak: 2,
                                },
                            },
                            "giftsub" => Alert {
                                new: true,
                                r#type: AlertEventType::GiftSub {
                                    gifter: String::from("lmao"),
                                    total: 9999,
                                    tier: String::from("3"),
                                },
                            },
                            _ => {
                                println!("no args provided");
                                Alert {
                                    new: true,
                                    r#type: AlertEventType::Raid {
                                        from: String::from("lmao"),
                                        viewers: 9999,
                                    },
                                }
                            }
                        };

                        match alerts_sender.send(alert) {
                            Ok(_) => {
                                ws_sender
                                    .send(Message::Text(to_irc_message(&format!(
                                        "Testing {} alert",
                                        if args.is_empty() { "raid" } else { args }
                                    ))))
                                    .await?;
                            }
                            Err(e) => {
                                println!("frontend event failed:: {e:?}");
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Message::Text(msg)) => {
                #[allow(clippy::if_same_then_else)]
                if msg.contains("PING") {
                    ws_sender
                        .send(Message::Text(String::from("PONG :tmi.twitch.tv\r\n")))
                        .await?;
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

fn mods_only(parsed_msg: &TwitchIrcMessage) -> Result<Option<String>, color_eyre::Report> {
    if !parsed_msg.tags.is_mod()? && !parsed_msg.tags.is_broadcaster()? {
        Ok(Some(String::from("This command can only be used by mods")))
    } else {
        Ok(None)
    }
}

#[derive(Default, Debug)]
struct Tags(HashMap<String, String>);

impl Tags {
    fn is_mod(&self) -> eyre::Result<bool> {
        match self.0.get("mod") {
            Some(r#mod) => Ok(r#mod == "1"),
            None => Err(eyre::eyre!("No mod tag")),
        }
    }
    fn is_broadcaster(&self) -> eyre::Result<bool> {
        match self.0.get("badges") {
            Some(badges) => Ok(badges.contains("broadcaster")),
            None => Err(eyre::eyre!("No badges tag")),
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
        // taken from:
        // https://github.com/robotty/twitch-irc-rs/blob/2e3e36b646630a0e6059896d7fece413180fb253/src/message/tags.rs#L10
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

#[derive(Default, Debug)]
struct TwitchIrcMessage {
    sender: String,
    tags: Tags,
    message: String,
}

impl From<HashMap<String, String>> for Tags {
    fn from(value: HashMap<String, String>) -> Self {
        Tags(value)
    }
}

pub fn to_irc_message(msg: &str) -> String {
    format!("PRIVMSG #sadmadladsalman :{}", msg)
}

fn parse_irc(msg: &str) -> TwitchIrcMessage {
    let (tags, message) = msg.split_once(' ').expect("sperate tags and message");

    let message = &message[1..];

    let sender = message.split_once('!').expect("sender").0.to_string();

    let message = message
        .split_once(':')
        .expect("message")
        .1
        .trim()
        .to_string();

    let tags = tags[1..]
        .split(';')
        .map(|s| {
            let (key, value) = s.split_once('=').expect("=");
            (
                key.to_string(),
                if value.contains("PRIVMSG") {
                    String::from("")
                } else {
                    value.to_string()
                },
            )
        })
        .collect::<HashMap<String, String>>()
        .into();

    TwitchIrcMessage {
        tags,
        message,
        sender,
    }
}
