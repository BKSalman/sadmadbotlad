use std::{
    collections::{hash_set::Difference, HashMap, HashSet},
    fmt::Display,
    fs,
    panic::{self, AssertUnwindSafe},
    path::Path,
    process,
    sync::Arc,
    time::Duration,
};

use crate::{
    commands::{run_hebi, Context},
    db::Store,
    flatten,
    song_requests::{play_song, setup_mpv, QueueMessages, SongRequest},
    twitch::{TwitchError, TwitchTokenMessages},
    Alert, CommandsError, CommandsLoader, MainError, APP, COMMANDS_PATH,
};
use futures::FutureExt;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt, TryFutureExt,
};
use libmpv::Mpv;
use notify::{RecommendedWatcher, Watcher};
use rand::seq::SliceRandom;
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
    irc_sender: mpsc::Sender<Message>,
    irc_receiver: mpsc::Receiver<Message>,
    alerts_sender: broadcast::Sender<Alert>,
    song_receiver: mpsc::Receiver<SongRequest>,
    queue_sender: mpsc::UnboundedSender<QueueMessages>,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    store: Arc<Store>,
) -> Result<(), IrcError> {
    println!("Starting IRC");

    let (socket, _) = connect_async("wss://irc-ws.chat.twitch.tv:443").await?;

    let (ws_sender, ws_receiver) = socket.split();

    let mpv = Arc::new(setup_mpv());

    let t_handle = {
        let mpv = mpv.clone();
        let queue_sender = queue_sender.clone();
        std::thread::spawn(move || play_song(mpv, song_receiver, queue_sender))
    };

    tokio::try_join!(flatten(tokio::spawn(
        read(
            irc_sender,
            irc_receiver,
            ws_receiver,
            ws_sender,
            queue_sender,
            token_sender,
            mpv,
            alerts_sender,
            store,
        )
        .map_err(Into::into)
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

async fn read(
    irc_sender: mpsc::Sender<Message>,
    mut irc_receiver: mpsc::Receiver<Message>,
    mut ws_receiver: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    mut ws_sender: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    queue_sender: mpsc::UnboundedSender<QueueMessages>,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    mpv: Arc<Mpv>,
    alerts_sender: broadcast::Sender<Alert>,
    store: Arc<Store>,
) -> Result<(), IrcError> {
    irc_login(&mut ws_sender, token_sender.clone()).await?;

    // TODO: move to separate task
    tokio::spawn(async move {
        while let Some(message) = irc_receiver.recv().await {
            ws_sender.send(message).await?;
        }

        Ok::<_, IrcError>(())
    });

    // let voters = Arc::new(RwLock::new(HashSet::new()));

    let commands = CommandsLoader::load_commands(COMMANDS_PATH)?;

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
                match CommandsLoader::load_commands(COMMANDS_PATH) {
                    Ok(new_commands) => *cloned_commands.lock().await = new_commands,
                    Err(error) => println!("Error reloading commands: {:#?}", error),
                }
            }
        }
    });

    watcher.watch(Path::new(COMMANDS_PATH), notify::RecursiveMode::Recursive)?;

    let mut vm = run_hebi(
        irc_sender.clone(),
        alerts_sender,
        token_sender.clone(),
        mpv,
        queue_sender,
    )
    .await?;

    'restart: loop {
        while let Some(msg) = ws_receiver.next().await {
            match msg {
                Ok(Message::Ping(ping)) => {
                    println!("IRC WebSocket Ping {ping:?}");
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
                                    eprintln!(
                                        "command: {command} \n-- code: {hebi_code} \n-- globals: {:#?} \n-- error: {vm_error}",
                                        vm.global().entries().collect::<Vec<_>>()
                                    );
                                }
                            }
                            Err(panic_err) => {
                                eprintln!("hebi panicked!");
                                eprintln!(
                                    "command: {command} \n-- code: {hebi_code} \n-- globals: {:#?}",
                                    vm.global().entries().collect::<Vec<_>>()
                                );
                                std::panic::panic_any(panic_err);
                            }
                        }
                    }

                    // match &command.to_lowercase()[1..] {
                    //     "ping" | "وكز" => {
                    //         if command.is_ascii() {
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(&format!(
                    //                     "{}pong",
                    //                     APP.config.cmd_delim
                    //                 ))))
                    //                 .await?;
                    //         } else {
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(&format!(
                    //                     "{}يكز",
                    //                     APP.config.cmd_delim
                    //                 ))))
                    //                 .await?;
                    //         }
                    //     }
                    //     "db" => {
                    //         let events = store.get_events().await?;

                    //         println!("{events:#?}");
                    //     }
                    //     "sr" | "طلب" => {
                    //         if args.is_empty() {
                    //             let e = format!("Correct usage: {}sr <URL>", APP.config.cmd_delim);
                    //             ws_sender.send(Message::Text(to_irc_message(&e))).await?;
                    //             continue;
                    //         }

                    //         let (send, recv) = oneshot::channel();

                    //         queue_sender.send(QueueMessages::Sr(
                    //             (parsed_msg.sender, args.to_string()),
                    //             send,
                    //         ))?;

                    //         let Ok(message) = recv.await else {
                    //         return Err(eyre::eyre!("Could not request song"));
                    //     };

                    //         ws_sender
                    //             .send(Message::Text(to_irc_message(&message)))
                    //             .await?;
                    //     }
                    //     "skip" | "تخطي" => {
                    //         if let Some(message) = parsed_msg.tags.mods_only()? {
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(&message)))
                    //                 .await?;
                    //             continue;
                    //         }

                    //         let (send, recv) = oneshot::channel();

                    //         queue_sender.send(QueueMessages::GetCurrentSong(send))?;

                    //         let Ok(current_song) = recv.await else {
                    //             return Err(eyre::eyre!("Could not get current song"));
                    //         };

                    //         let mut message = String::new();

                    //         if let Some(song) = current_song {
                    //             if mpv.playlist_next_force().is_ok() {
                    //                 message = format!("Skipped: {}", song.title);
                    //             }
                    //         } else {
                    //             message = String::from("No song playing");
                    //         }

                    //         ws_sender
                    //             .send(Message::Text(to_irc_message(&message)))
                    //             .await?;
                    //     }
                    //     "voteskip" | "تصويت-تخطي" => {
                    //         // TODO: figure out how to make every vote reset the timer (timeout??)

                    //         let votersc = voters.clone();

                    //         let mut t_handle: Option<JoinHandle<()>> = None;

                    //         if voters.read().await.len() == 0 {
                    //             t_handle = Some(tokio::spawn(async move {
                    //                 tokio::time::sleep(Duration::from_secs(20)).await;
                    //                 votersc.write().await.clear();
                    //                 println!("reset counter");
                    //             }));
                    //         }

                    //         voters.write().await.insert(parsed_msg.sender);

                    //         println!("{}", voters.read().await.len());

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
                    //     "queue" | "q" | "السرا" => {
                    //         ws_sender
                    //             .send(Message::Text(to_irc_message(
                    //                 "Check the queue at: https://f5rfm.bksalman.com",
                    //             )))
                    //             .await?;
                    //     }
                    //     "currentsong" | "song" | "الحالية" | "الاغنية" | "الاغنيةالحالية" =>
                    //     {
                    //         let (send, recv) = oneshot::channel();

                    //         queue_sender.send(QueueMessages::GetCurrentSong(send))?;

                    //         let Ok(current_song) = recv.await else {
                    //         return Err(eyre::eyre!("Could not get current song"));
                    //     };

                    //         let message = if let Some(song) = current_song {
                    //             format!(
                    //                 "current song: {} , requested by {} - {}",
                    //                 song.title, song.user, song.url,
                    //             )
                    //         } else {
                    //             String::from("No song playing")
                    //         };

                    //         ws_sender
                    //             .send(Message::Text(to_irc_message(&message)))
                    //             .await?;
                    //     }
                    //     "currentspotify" | "currentsp" => {
                    //         let cmd = process::Command::new("playerctl")
                    //             .args([
                    //                 "--player=spotify",
                    //                 "metadata",
                    //                 "--format",
                    //                 "{{title}} - {{artist}}",
                    //             ])
                    //             .output()?;
                    //         let output = String::from_utf8(cmd.stdout)?;

                    //         ws_sender
                    //             .send(Message::Text(to_irc_message(&format!(
                    //                 "Current Spotify song: {}",
                    //                 output
                    //             ))))
                    //             .await?;
                    //     }
                    //     "volume" | "v" => {
                    //         if args.is_empty() {
                    //             let Ok(volume) = mpv.get_property::<i64>("volume") else {
                    //                             println!("volume error");
                    //                             ws_sender.send(Message::Text(to_irc_message("No volume"))).await?;
                    //                             return Ok(());
                    //                         };
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(&format!(
                    //                     "Volume: {}",
                    //                     volume
                    //                 ))))
                    //                 .await?;
                    //             continue;
                    //         }

                    //         if let Some(message) = parsed_msg.tags.mods_only()? {
                    //             ws_sender.send(Message::Text(message)).await?;
                    //             continue;
                    //         }

                    //         let Ok(volume) = args.parse::<i64>() else {
                    //             let e = String::from("Provide number");
                    //             ws_sender.send(Message::Text(e)).await?;
                    //             continue;
                    //         };

                    //         if volume > 100 {
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message("Max volume is 100")))
                    //                 .await?;
                    //             continue;
                    //         }

                    //         if let Err(e) = mpv.set_property("volume", volume) {
                    //             println!("{e}");
                    //         }

                    //         ws_sender
                    //             .send(Message::Text(to_irc_message(&format!(
                    //                 "Volume set to {}",
                    //                 volume
                    //             ))))
                    //             .await?;
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
                    //                 println!("{e}");
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
                    //             println!("no script");
                    //         }
                    //     }
                    //     "stop" | "قف" | "وقف" => {
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
                    //             if let Err(e) = mpv.pause() {
                    //                 println!("{e}");
                    //                 continue;
                    //             }

                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(&format!(
                    //                     "Stopped playing {}, !play to resume",
                    //                     song.title
                    //                 ))))
                    //                 .await?;
                    //         } else {
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message("Queue is empty")))
                    //                 .await?;
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
                    //             println!("{e}");
                    //             continue;
                    //         }

                    //         ws_sender
                    //             .send(Message::Text(to_irc_message("Stopped playing spotify")))
                    //             .await?;
                    //     }
                    //     "قوانين" => {
                    //         if let Some(message) = parsed_msg.tags.mods_only()? {
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(&message)))
                    //                 .await?;
                    //             continue;
                    //         }

                    //         for rule in RULES.lines() {
                    //             ws_sender.send(Message::Text(to_irc_message(rule))).await?;
                    //         }
                    //     }
                    //     "title" | "عنوان" => {
                    //         if args.is_empty() {
                    //             let title = get_title(token_sender.clone()).await?;

                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(&format!(
                    //                     "Current stream title: {}",
                    //                     title
                    //                 ))))
                    //                 .await?;

                    //             continue;
                    //         }

                    //         if let Some(message) = parsed_msg.tags.mods_only()? {
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(&message)))
                    //                 .await?;
                    //             continue;
                    //         }

                    //         set_title(args, token_sender.clone()).await?;
                    //         ws_sender
                    //             .send(Message::Text(to_irc_message(&format!(
                    //                 "Set stream title to: {}",
                    //                 args
                    //             ))))
                    //             .await?;
                    //     }
                    //     "warranty" | "تأمين" => {
                    //         ws_sender
                    //             .send(Message::Text(to_irc_message(WARRANTY)))
                    //             .await?;
                    //         continue;
                    //     }
                    //     "rustwarranty" | "!rwarranty" | "تأمين-صدأ" => {
                    //         ws_sender
                    //             .send(Message::Text(to_irc_message(RUST_WARRANTY)))
                    //             .await?;
                    //         continue;
                    //     }
                    //     "workingon" | "wo" => {
                    //         if let Some(message) = parsed_msg.tags.mods_only()? {
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(&message)))
                    //                 .await?;
                    //             continue;
                    //         }

                    //         fs::write(
                    //             String::from("workingon.txt"),
                    //             format!("Currently: {}", args),
                    //         )?;

                    //         ws_sender
                    //             .send(Message::Text(to_irc_message(&format!(
                    //                 "Updated current working on to \"{}\"",
                    //                 args
                    //             ))))
                    //             .await?;
                    //     }
                    //     "pixelperfect" | "pp" => {
                    //         ws_sender
                    //             .send(Message::Text(to_irc_message("image_res.rect")))
                    //             .await?;
                    //     }
                    //     "discord" | "disc" => {
                    //         ws_sender
                    //             .send(Message::Text(to_irc_message("https://discord.gg/qs4SGUt")))
                    //             .await?;
                    //     }
                    //     "nerd" => {
                    //         if let Ok(reply) = parsed_msg.tags.get_reply() {
                    //             let message = format!("Nerd \"{}\"", reply);
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(message)))
                    //                 .await?;
                    //             continue;
                    //         }

                    //         ws_sender
                    //             .send(Message::Text(String::from(
                    //                 "Reply to a message to use this command",
                    //             )))
                    //             .await?;
                    //     }
                    //     "7tv" => {
                    //         ws_sender
                    //             .send(Message::Text(to_irc_message(&format!(
                    //                 "https://7tv.app/emotes?query={args}"
                    //             ))))
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
                    //     "test" => {
                    //         if let Some(message) = parsed_msg.tags.mods_only()? {
                    //             ws_sender
                    //                 .send(Message::Text(to_irc_message(&message)))
                    //                 .await?;
                    //             continue;
                    //         }

                    //         let alert = match args.to_lowercase().as_str() {
                    //             "raid" => Alert::raid_test(),
                    //             "follow" => Alert::follow_test(),
                    //             "sub" => Alert::sub_test(),
                    //             "resub" => Alert::resub_test(),
                    //             "giftsub" => Alert::giftsub_test(),
                    //             "asd" => Alert::asd_test(),
                    //             _ => {
                    //                 println!("no args provided");
                    //                 Alert {
                    //                     new: true,
                    //                     r#type: AlertEventType::Raid {
                    //                         from: String::from("lmao"),
                    //                         viewers: 9999,
                    //                     },
                    //                 }
                    //             }
                    //         };

                    //         match alerts_sender.send(alert) {
                    //             Ok(_) => {
                    //                 ws_sender
                    //                     .send(Message::Text(to_irc_message(&format!(
                    //                         "Testing {} alert",
                    //                         if args.is_empty() { "raid" } else { args }
                    //                     ))))
                    //                     .await?;
                    //             }
                    //             Err(e) => {
                    //                 println!("frontend event failed:: {e:?}");
                    //             }
                    //         }
                    //     }
                    //     _ => {}
                    // }
                }
                Ok(Message::Text(msg)) => {
                    #[allow(clippy::if_same_then_else)]
                    if msg.contains("PING") {
                        irc_sender
                            .send(Message::Text(String::from("PONG :tmi.twitch.tv\r\n")))
                            .await?;
                    } else if msg.contains("RECONNECT") {
                        // TODO: reconnect
                        continue 'restart;
                    } else {
                        // println!("{msg}");
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    return Err(IrcError::WsError(e));
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
