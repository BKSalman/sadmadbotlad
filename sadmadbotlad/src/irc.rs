use std::{collections::HashMap, fmt::Display, panic::AssertUnwindSafe, path::Path, sync::Arc};

use crate::{
    APP, Alert, CommandsError, CommandsLoader,
    commands::{Context, run_hebi},
    db::DBMessage,
    song_requests::{QueueMessages, SongRequestsError},
    twitch::{TwitchError, TwitchTokenMessages},
};
use futures::FutureExt;
use futures_util::{SinkExt, StreamExt};
use libmpv::Mpv;
use notify::{RecommendedWatcher, Watcher};
use tokio::sync::{
    broadcast,
    mpsc::{self, UnboundedSender},
    oneshot,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

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
    Anyhow(#[from] anyhow::Error),

    #[error("no badges tag")]
    NoBadgeTag,

    #[error("no mod tag")]
    NoModTag,

    #[error("no reply tag")]
    NoReplyTag,

    #[error(transparent)]
    SongRequest(#[from] SongRequestsError),
}

pub async fn irc_connect(
    alerts_sender: broadcast::Sender<Alert>,
    queue_sender: mpsc::UnboundedSender<QueueMessages>,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    db_tx: std::sync::mpsc::Sender<DBMessage>,
    mpv: Arc<Mpv>,
) -> Result<(), IrcError> {
    tracing::info!("Starting IRC");

    read(queue_sender, token_sender, mpv, alerts_sender, db_tx).await?;

    Ok(())
}

pub async fn irc_login(
    irc_sender: tokio::sync::mpsc::Sender<Message>,
    token_sender: UnboundedSender<TwitchTokenMessages>,
) -> Result<(), IrcError> {
    let cap = Message::Text(String::from("CAP REQ :twitch.tv/commands twitch.tv/tags").into());

    irc_sender.send(cap).await?;

    let (one_shot_sender, one_shot_receiver) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(one_shot_sender))?;

    let Ok(api_info) = one_shot_receiver.await else {
        return Err(IrcError::TwitchError(TwitchError::TokenError));
    };

    let pass_msg = Message::Text(format!("PASS oauth:{}", api_info.twitch_access_token).into());

    irc_sender.send(pass_msg).await?;

    let nick_msg = Message::Text(format!("NICK {}", api_info.user).into());

    irc_sender.send(nick_msg).await?;

    irc_sender
        .send(Message::Text(String::from("JOIN #sadmadladsalman").into()))
        .await?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn read(
    queue_sender: mpsc::UnboundedSender<QueueMessages>,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    mpv: Arc<Mpv>,
    alerts_sender: broadcast::Sender<Alert>,
    _db_tx: std::sync::mpsc::Sender<DBMessage>,
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

        let (irc_sender, mut irc_receiver) = tokio::sync::mpsc::channel::<Message>(200);

        tokio::spawn(async move {
            while let Some(message) = irc_receiver.recv().await {
                if let Err(e) = ws_sender.send(message).await {
                    tracing::error!("Failed to send on twitch IRC websocket: {e}");
                }
            }
        });

        irc_login(irc_sender.clone(), token_sender.clone()).await?;

        let mut vm = run_hebi(
            irc_sender.clone(),
            alerts_sender.clone(),
            token_sender.clone(),
            mpv.clone(),
            queue_sender.clone(),
        )
        .await?;

        while let Some(msg) = ws_receiver.next().await {
            match msg {
                Ok(Message::Ping(ping)) => {
                    tracing::debug!("IRC WebSocket Ping {ping:?}");
                    irc_sender
                        .send(Message::Pong(tokio_tungstenite::tungstenite::Bytes::new()))
                        .await?;
                }
                Ok(Message::Text(msg)) if msg.contains("PRIVMSG") => {
                    let parsed_msg = parse_irc(&msg);

                    let message = if parsed_msg.tags.get_reply().is_some() {
                        // remove mention
                        parsed_msg
                            .message
                            .split_once(' ')
                            .expect("remove mention")
                            .1
                    } else {
                        &parsed_msg.message
                    };

                    if !message.starts_with(APP.config.cmd_delim) {
                        continue;
                    }

                    let (command, args) = message.split_once(' ').unwrap_or((message, ""));

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
                }
                Ok(Message::Text(msg)) => {
                    if msg.starts_with("PING :tmi.twitch.tv") {
                        irc_sender
                            .send(Message::Text(
                                String::from("PONG :tmi.twitch.tv\r\n").into(),
                            ))
                            .await?;
                    } else if msg.contains(":tmi.twitch.tv RECONNECT") {
                        tracing::debug!("reconnect");
                        continue 'restart;
                    } else {
                        tracing::info!("received text message: {msg}");
                    }
                }
                Ok(Message::Close(c)) => {
                    tracing::debug!("IRC connection closed, reason: {c:?}");
                    continue 'restart;
                }
                Ok(_) => {
                    // received something weird, just restart I guess
                    continue 'restart;
                }
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
