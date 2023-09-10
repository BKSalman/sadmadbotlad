use std::{collections::HashMap, fmt::Display, panic::AssertUnwindSafe, path::Path, sync::Arc};

use crate::{
    commands::{run_hebi, Context},
    db::Store,
    flatten,
    song_requests::{play_song, setup_mpv, QueueMessages, SongRequest},
    twitch::{TwitchError, TwitchTokenMessages},
    Alert, CommandsError, CommandsLoader, MainError, APP,
};
use futures::FutureExt;
use futures_util::{stream::SplitSink, SinkExt, StreamExt, TryFutureExt};
use libmpv::Mpv;
use notify::{RecommendedWatcher, Watcher};
use tokio::{
    net::TcpStream,
    sync::{
        broadcast,
        mpsc::{self, UnboundedSender},
        oneshot,
    },
};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

#[derive(thiserror::Error, Debug)]
pub enum IrcError {
    #[error(transparent)]
    TwitchError(#[from] TwitchError),

    #[error(transparent)]
    CommandsError(#[from] CommandsError),

    #[error(transparent)]
    WsError(#[from] tokio_tungstenite::tungstenite::Error),

    #[error(transparent)]
    TwitchTokenSendError(#[from] tokio::sync::mpsc::error::SendError<TwitchTokenMessages>),

    #[error(transparent)]
    WsSendError(#[from] tokio::sync::mpsc::error::SendError<Message>),

    #[error(transparent)]
    NotifyError(#[from] notify::Error),

    #[error(transparent)]
    HebiError(#[from] hebi::Error),

    #[error(transparent)]
    MainError(#[from] MainError),

    #[error("no badges tag")]
    NoBadgeTag,

    #[error("no mod tag")]
    NoModTag,

    #[error("no reply tag")]
    NoReplyTag,
}

pub async fn irc_connect(
    alerts_sender: broadcast::Sender<Alert>,
    song_receiver: mpsc::Receiver<SongRequest>,
    queue_sender: mpsc::UnboundedSender<QueueMessages>,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    store: Arc<Store>,
) -> Result<(), IrcError> {
    tracing::info!("Starting IRC");

    let mpv = Arc::new(setup_mpv());

    let t_handle = {
        let mpv = mpv.clone();
        let queue_sender = queue_sender.clone();
        std::thread::spawn(move || play_song(mpv, song_receiver, queue_sender))
    };

    tokio::try_join!(flatten(tokio::spawn(
        read(queue_sender, token_sender, mpv, alerts_sender, store,).map_err(Into::into)
    )))?;

    let _ = t_handle.join().expect("play_song thread");

    Ok(())
}

pub async fn irc_login(
    ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    token_sender: UnboundedSender<TwitchTokenMessages>,
) -> Result<(), IrcError> {
    let cap = Message::Text(String::from("CAP REQ :twitch.tv/commands twitch.tv/tags"));

    ws_sender.send(cap).await?;

    let (one_shot_sender, one_shot_receiver) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(one_shot_sender))?;

    let Ok(api_info) = one_shot_receiver.await else {
        return Err(IrcError::TwitchError(TwitchError::TokenError));
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

#[allow(clippy::too_many_arguments)]
async fn read(
    queue_sender: mpsc::UnboundedSender<QueueMessages>,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    mpv: Arc<Mpv>,
    alerts_sender: broadcast::Sender<Alert>,
    _store: Arc<Store>,
) -> Result<(), IrcError> {
    // let voters = Arc::new(RwLock::new(HashSet::new()));

    let commands = CommandsLoader::load_commands(&APP.config.commands_path)?;

    let commands = Arc::new(tokio::sync::Mutex::new(commands));

    let cloned_commands = Arc::clone(&commands);

    let (watcher_sender, mut watcher_receiver) =
        tokio::sync::mpsc::channel::<Result<notify::Event, notify::Error>>(10);

    let mut watcher = RecommendedWatcher::new(
        move |watcher: Result<notify::Event, notify::Error>| {
            watcher_sender.blocking_send(watcher).expect("send watcher");
        },
        notify::Config::default(),
    )?;

    tokio::spawn(async move {
        while let Some(watcher) = watcher_receiver.recv().await {
            let event = watcher.unwrap();

            if event.kind.is_modify() {
                match CommandsLoader::load_commands(&APP.config.commands_path) {
                    Ok(new_commands) => *cloned_commands.lock().await = new_commands,
                    Err(error) => tracing::error!("Error reloading commands: {:#?}", error),
                }
            }
        }
    });

    watcher.watch(
        Path::new(&APP.config.commands_path),
        notify::RecursiveMode::Recursive,
    )?;

    'restart: loop {
        tracing::debug!("irc 'restart loop");

        let (socket, _) = connect_async("wss://irc-ws.chat.twitch.tv:443").await?;

        let (mut ws_sender, mut ws_receiver) = socket.split();

        irc_login(&mut ws_sender, token_sender.clone()).await?;

        let (irc_sender, mut irc_receiver) = tokio::sync::mpsc::channel::<Message>(200);

        let mut vm = run_hebi(
            irc_sender.clone(),
            alerts_sender.clone(),
            token_sender.clone(),
            mpv.clone(),
            queue_sender.clone(),
        )
        .await?;

        tokio::spawn(async move {
            while let Some(message) = irc_receiver.recv().await {
                ws_sender.send(message).await?;
            }

            Ok::<_, IrcError>(())
        });

        while let Some(msg) = ws_receiver.next().await {
            match msg {
                Ok(Message::Ping(ping)) => {
                    tracing::debug!("IRC WebSocket Ping {ping:?}");
                    irc_sender.send(Message::Pong(vec![])).await?;
                }
                Ok(Message::Text(msg)) if msg.contains("PRIVMSG") => {
                    let parsed_msg = parse_irc(&msg);

                    let (command, args) = if parsed_msg.tags.get_reply().is_some() {
                        // remove mention
                        let (_, message) =
                            parsed_msg.message.split_once(' ').expect("remove mention");

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

                    let locked_commands = commands.lock().await;

                    if &command.to_lowercase()[1..] == "reconnect" {
                        continue 'restart;
                    }

                    if let Some(hebi_code) = locked_commands.get(&command.to_lowercase()[1..]) {
                        vm.global().set(
                            vm.new_string("ctx"),
                            vm.new_instance(Context {
                                args: args.split_whitespace().map(|s| s.to_string()).collect(),
                                message_metadata: parsed_msg.clone(),
                            })?,
                        );

                        let result = AssertUnwindSafe(vm.eval_async(hebi_code))
                            .catch_unwind()
                            .await;

                        match result {
                            Ok(eval_result) => {
                                if let Err(vm_error) = eval_result {
                                    tracing::error!(
                                        "command: {command} \n-- code: {hebi_code} \n-- globals: {:#?} \n-- error: {vm_error}",
                                        vm.global().entries().collect::<Vec<_>>()
                                    );
                                }
                            }
                            Err(panic_err) => {
                                tracing::error!("Hebi panicked!");
                                tracing::error!(
                                    "command: {command} \n-- code: {hebi_code} \n-- globals: {:#?}",
                                    vm.global().entries().collect::<Vec<_>>()
                                );
                                tracing::error!("Hebi panicked!");
                                std::panic::panic_any(panic_err);
                            }
                        }
                    }

                    // match &command.to_lowercase()[1..] {
                    //     "voteskip" | "تصويت-تخطي" => {
                    //         // TODO: figure out how to make every vote reset the timer (timeout??)

                    //         let votersc = voters.clone();

                    //         let mut t_handle: Option<JoinHandle<()>> = None;

                    //         if voters.read().await.len() == 0 {
                    //             t_handle = Some(tokio::spawn(async move {
                    //                 tokio::time::sleep(Duration::from_secs(20)).await;
                    //                 votersc.write().await.clear();
                    //                 tracing::debug!("reset counter");
                    //             }));
                    //         }

                    //         voters.write().await.insert(parsed_msg.sender);

                    //         tracing::debug!("{}", voters.read().await.len());

                    //         ws_sender
                    //             .send(Message::Text(to_irc_message(&format!(
                    //                 "{}/4 skip votes",
                    //                 voters.read().await.len()
                    //             ))))
                    //             .await?;

                    //         if voters.read().await.len() >= 4 {
                    //             voters.write().await.clear();

                    //             let (send, recv) = oneshot::channel();

                    //             queue_sender.send(QueueMessages::GetCurrentSong(send))?;

                    //             let Ok(current_song) = recv.await else {
                    //             return Err(eyre::eyre!("Could not get current song"));
                    //         };

                    //             let message = if let Some(song) = current_song {
                    //                 queue_sender.send(QueueMessages::Dequeue)?;

                    //                 if let Some(t_handle) = t_handle {
                    //                     t_handle.abort();
                    //                 }

                    //                 if mpv.playlist_next_force().is_ok() {
                    //                     format!("Skipped: {}", song.title)
                    //                 } else {
                    //                     String::from("lmao")
                    //                 }
                    //             } else {
                    //                 String::from("No song playing")
                    //             };

                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(&message)))
                    //                 .await?;
                    //         }
                    //     }
                    //     "play" | "إبدأ" | "ابدأ" | "ابدا" | "بدء" => {
                    //         if let Some(message) = parsed_msg.tags.mods_only()? {
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(&message)))
                    //                 .await?;
                    //             continue;
                    //         }

                    //         let (send, recv) = oneshot::channel();

                    //         queue_sender.send(QueueMessages::GetCurrentSong(send))?;

                    //         let Ok(current_song) = recv.await else {
                    //         return Err(eyre::eyre!("Could not get current song"));
                    //     };

                    //         if let Some(song) = current_song {
                    //             if let Err(e) = mpv.unpause() {
                    //                 tracing::error!("{e}");
                    //             }

                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(&format!(
                    //                     "Resumed {}",
                    //                     song.title
                    //                 ))))
                    //                 .await?;
                    //         } else {
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message("Queue is empty")))
                    //                 .await?;
                    //         }
                    //     }
                    //     "playspotify" | "playsp" => {
                    //         if let Some(message) = parsed_msg.tags.mods_only()? {
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(&message)))
                    //                 .await?;
                    //             continue;
                    //         }

                    //         if process::Command::new("playerctl")
                    //             .args(["--player=spotify", "play"])
                    //             .spawn()
                    //             .is_ok()
                    //         {
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message("Started playing spotify")))
                    //                 .await?;
                    //         } else {
                    //             tracing::error!("no script");
                    //         }
                    //     }
                    //     "stopspotify" | "stopsp" => {
                    //         if let Some(message) = parsed_msg.tags.mods_only()? {
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(&message)))
                    //                 .await?;
                    //             continue;
                    //         }

                    //         if let Err(e) = process::Command::new("playerctl")
                    //             .args(["--player=spotify", "pause"])
                    //             .spawn()
                    //         {
                    //             tracing::error!("{e}");
                    //             continue;
                    //         }

                    //         ws_sender
                    //             .send(Message::Text(to_irc_message("Stopped playing spotify")))
                    //             .await?;
                    //     }
                    //     "hard" => {
                    //         ws_sender
                    //             .send(Message::Text(to_irc_message(
                    //                 "There are 2 hard problems in computer science: cache invalidation, naming things, and off-by-1 errors."
                    //             )))
                    //             .await?;
                    //     }
                    //     "roll" => {
                    //         if args.trim().is_empty() {
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(
                    //                     "Usage: !roll <choice one> | <choice two> | <choice three>",
                    //                 )))
                    //                 .await?;
                    //             continue;
                    //         }
                    //         let options: Vec<&str> = args.split('|').collect();

                    //         let Some(chosen) = options.choose(&mut rand::thread_rng()) else {
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message("No choices were provided")))
                    //                 .await?;
                    //             continue;
                    //         };

                    //         ws_sender
                    //             .send(Message::Text(to_irc_message(&format!(
                    //                 r#""{}" Won!"#,
                    //                 chosen.trim()
                    //             ))))
                    //             .await?;
                    //     }
                    //     _ => {}
                    // }
                }
                Ok(Message::Text(msg)) => {
                    if msg.contains("PING") {
                        irc_sender
                            .send(Message::Text(String::from("PONG :tmi.twitch.tv\r\n")))
                            .await?;
                    } else if msg.contains("RECONNECT") {
                        tracing::debug!("reconnect with url");
                        continue 'restart;
                    } else {
                        // tracing::info!("{msg}");
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::error!("IRC websocket error: {e}");
                    continue 'restart;
                }
            }
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct Tags(HashMap<String, String>);

impl std::ops::Deref for Tags {
    type Target = HashMap<String, String>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Tags {
    pub fn mods_only(&self) -> Option<String> {
        if !self.is_mod()? && !self.is_broadcaster()? {
            Some(String::from("This command can only be used by mods"))
        } else {
            None
        }
    }
    pub fn is_mod(&self) -> Option<bool> {
        if let Some(r#mod) = self.0.get("mod") {
            return Some(r#mod == "1");
        }
        None
    }
    pub fn is_broadcaster(&self) -> Option<bool> {
        if let Some(badges) = self.0.get("badges") {
            return Some(badges.contains("broadcaster"));
        }
        None
    }
    pub fn get_reply(&self) -> Option<String> {
        if let Some(msg) = self.0.get("reply-parent-msg-body") {
            return Some(Self::decode_message(msg));
        }
        None
    }
    pub fn decode_message(msg: &str) -> String {
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
    pub fn get_sender(&self) -> Option<String> {
        self.0.get("display-name").map(|s| s.to_string())
    }
}

#[derive(Default, Debug, Clone)]
pub struct TwitchIrcMessage {
    pub tags: Tags,
    pub message: String,
}

impl From<HashMap<String, String>> for Tags {
    fn from(value: HashMap<String, String>) -> Self {
        Tags(value)
    }
}

pub fn to_irc_message(msg: impl Display) -> String {
    format!("PRIVMSG #sadmadladsalman :{}", msg)
}

fn parse_irc(msg: &str) -> TwitchIrcMessage {
    let (tags, message) = msg.split_once(' ').expect("sperate tags and message");

    let message = &message[1..];

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

    TwitchIrcMessage { tags, message }
}
