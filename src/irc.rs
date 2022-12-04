use eyre::WrapErr;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use libmpv::Mpv;
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use sadmadbotlad::twitch;
use sadmadbotlad::{flatten, ApiInfo};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::{
    net::TcpStream,
    sync::{
        mpsc::{self, Sender},
        MutexGuard,
    },
};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::song_requests::{play_song, Queue, SongRequest, SongRequestSetup};

const FRAGMENT: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'<').add(b'>').add(b'`');

pub async fn irc_connect() -> eyre::Result<()> {
    println!("Starting IRC");

    let api_info = ApiInfo::new();

    let (socket, _) = connect_async("wss://irc-ws.chat.twitch.tv:443").await?;

    let (ws_sender, ws_receiver) = socket.split();

    let ws_sender = Arc::new(tokio::sync::Mutex::new(ws_sender));

    let ws_sender_ref = ws_sender.clone();

    login(ws_sender_ref.lock().await, &api_info).await?;

    let (sender, receiver) = mpsc::channel::<SongRequest>(200);

    let sr_setup = SongRequestSetup::new();

    let queue = sr_setup.queue.clone();

    let mpv = sr_setup.mpv.clone();

    let join_handle = std::thread::spawn(move || play_song(receiver, mpv, queue));

    tokio::try_join!(flatten(tokio::spawn(async move {
        read(
            sender,
            ws_sender,
            ws_receiver,
            sr_setup.mpv,
            sr_setup.queue,
            &api_info,
        )
        .await
    })))
    .wrap_err_with(|| "songs")?;

    join_handle.join().expect("thread join");

    Ok(())
}

async fn login<'a>(
    mut locked_sender: MutexGuard<
        'a,
        SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    >,
    api_info: &ApiInfo,
) -> Result<(), eyre::Report> {
    let pass_msg = Message::Text(format!("PASS oauth:{}", api_info.twitch_oauth));

    locked_sender.send(pass_msg).await?;

    let nick_msg = Message::Text(format!("NICK {}", api_info.user));

    locked_sender.send(nick_msg).await?;

    locked_sender
        .send(Message::Text(String::from("JOIN #sadmadladsalman")))
        .await?;

    Ok(())
}

async fn read(
    sender: Sender<SongRequest>,
    ws_sender: Arc<
        tokio::sync::Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>,
    >,
    mut receiver: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    mpv: Arc<Mpv>,
    queue: Arc<Mutex<Queue>>,
    api_info: &ApiInfo,
) -> eyre::Result<()> {
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(msg)) => {
                if msg.contains("PRIVMSG") {
                    let parsed_msg = parse_message(&msg);

                    if !parsed_msg.starts_with('!') {
                        continue;
                    }

                    let parsed_sender = parse_sender(&msg);

                    println!("{}: {}", parsed_sender, parsed_msg);

                    let mut message = String::new();

                    if parsed_msg.starts_with("!ping") {
                        // abstract this to a function that sends messages to the chat
                        message = String::from("PRIVMSG #sadmadladsalman :!pong");

                        println!("!pong");
                    } else if parsed_msg.starts_with("!title ")
                        && parsed_sender.to_ascii_lowercase() == "sadmadladsalman"
                    {
                        twitch::change_title(&parsed_msg, api_info).await?;
                        message = format!("PRIVMSG #sadmadladsalman :Changed title to {}", parsed_msg);
                    } else if parsed_msg.starts_with("!title")
                    {
                        message = format!("PRIVMSG #sadmadladsalman :Title: {}", twitch::get_title(api_info).await?);
                    } else if parsed_msg.starts_with("!sr ") {
                        let mut song_msg = parsed_msg.splitn(2, ' ').collect::<Vec<&str>>()[1];
                        let http_client = reqwest::Client::new();

                        // TODO: move this to a seprate function you lazy ass bitch
                        if !song_msg.starts_with("https://") {
                            let res = http_client
                                .get(format!(
                                    "https://youtube.googleapis.com/youtube/v3/\
                                    search?part=snippet&maxResults=1&q={}&type=video&key={}",
                                    utf8_percent_encode(&song_msg, FRAGMENT),
                                    api_info.google_api_key
                                ))
                                .send()
                                .await?
                                .json::<Value>()
                                .await?;

                            let video_title = res["items"][0]["snippet"]["title"]
                                .as_str()
                                .expect("yt video title");

                            let video_id = res["items"][0]["id"]["videoId"]
                                .as_str()
                                .expect("yt video id");

                            let song = SongRequest {
                                user: parsed_sender,
                                song_url: format!("https://www.youtube.com/watch/{}", video_id),
                            };

                            sender.send(song.clone()).await.expect("send song");

                            queue
                                .lock()
                                .expect("read:: queue lock")
                                .enqueue(song.clone())
                                .expect("Enqueuing");

                            message = format!("PRIVMSG #sadmadladsalman :Added: {}", video_title);
                        } else {
                            if song_msg.contains("?v=") && song_msg.contains("&") {
                                song_msg = &song_msg[song_msg.find("?v=").unwrap() + 3
                                    ..song_msg.find('&').unwrap()];
                            } else if song_msg.contains("?v=") {
                                song_msg = &song_msg[song_msg.find("?v=").unwrap() + 3..];
                            } else if song_msg.contains("/watch/") {
                                song_msg =
                                    song_msg.rsplit_once('/').expect("yt watch format link").1;
                            }

                            let res = http_client
                                .get(format!(
                                    "https://youtube.googleapis.com/youtube/v3/\
                                    search?part=snippet&maxResults=1&q={}&type=video&key={}",
                                    song_msg, api_info.google_api_key
                                ))
                                .send()
                                .await?
                                .json::<Value>()
                                .await?;

                            let video_title = res["items"][0]["snippet"]["title"]
                                .as_str()
                                .expect("yt video title");

                            let song = SongRequest {
                                user: parsed_sender,
                                song_url: format!("https://youtube.com/watch/{}", song_msg),
                            };

                            sender.send(song.clone()).await.expect("send song");

                            queue
                                .lock()
                                .expect("read:: queue lock")
                                .enqueue(song.clone())
                                .expect("Enqueuing");

                            message = format!("PRIVMSG #sadmadladsalman :Added: {}", video_title);
                        }
                    } else if parsed_msg.starts_with("!skip") {
                        if let Err(e) = mpv.playlist_next_force() {
                            println!("{e}");
                        }

                        message = format!(
                            "PRIVMSG #sadmadladsalman :Skipped {}",
                            queue
                                .lock()
                                .expect("read:: queue")
                                .current_song
                                .as_ref()
                                .expect("read::!skip: current song")
                                .song_url
                        );

                        if queue.lock().expect("read:: queue").rear == 0 {
                            queue.lock().expect("read:: queue").current_song = None;
                        }
                    } else if parsed_msg.starts_with("!volume ") {
                        let Ok(value) = parsed_msg.split(' ').collect::<Vec<&str>>()[1].parse::<i64>() else {
                            continue;
                        };

                        if let Err(e) = mpv.set_property("volume", value) {
                            println!("{e}");
                        }

                        message = format!("PRIVMSG #sadmadladsalman :Volume set to {value}",);
                    } else if parsed_msg.starts_with("!volume") {
                        let Ok(volume) = mpv.get_property::<i64>("volume") else {
                            panic!("read:: volume");
                        };

                        message = format!("PRIVMSG #sadmadladsalman :Volume: {}", volume);
                    } else if parsed_msg.starts_with("!pause") {
                        if let Err(e) = mpv.pause() {
                            println!("{e}");
                        }

                        message = String::from("PRIVMSG #sadmadladsalman :Paused");
                    } else if parsed_msg.starts_with("!play") {
                        if let Some(something) =
                            &queue.lock().expect("read::!play: queue lock").current_song
                        {
                            if let Err(e) = mpv.unpause() {
                                println!("{e}");
                            }

                            message = format!("PRIVMSG #sadmadladsalman :Resumed {:?}", something);
                        } else {
                            message = String::from("PRIVMSG #sadmadladsalman :Queue is empty");
                        }
                    } else if parsed_msg.starts_with("!queue") {
                        let locked_queue = queue.lock().expect("read::!queue: queue lock");

                        let mut queue = String::new();

                        for (s, i) in locked_queue.clone().queue.iter().zip(1..=20) {
                            if let Some(song) = s {
                                queue.push_str(&format!(
                                    " {i}- {} :: by {}",
                                    song.song_url, song.user,
                                ));
                            }
                        }

                        println!("{:#?}", locked_queue);

                        if queue.chars().count() <= 0 {
                            message = format!("PRIVMSG #sadmadladsalman :Queue is empty");
                        } else {
                            message = format!("PRIVMSG #sadmadladsalman :Queue:{}", queue);
                        }
                    } else if parsed_msg.starts_with("!currentsong") {
                        let locked_queue = queue.lock().expect("read:: queue");

                        println!("{locked_queue:#?}");

                        if let Some(current_song) = locked_queue.current_song.as_ref() {
                            message = format!(
                                "PRIVMSG #sadmadladsalman :Current song: {} - by {}",
                                current_song.song_url, current_song.user,
                            );
                        } else {
                            message = String::from("PRIVMSG #sadmadladsalman :No song playing");
                        }
                    }

                    let mut locked_sender = ws_sender.lock().await;

                    locked_sender
                        .send(Message::Text(message))
                        .await
                        .expect("read::sr: chat");
                } else if msg.contains("RECONNECT") {
                    println!("Reconnecting IRC");

                    let mut locked_sender = ws_sender.lock().await;

                    let (socket, _) = connect_async("wss://irc-ws.chat.twitch.tv:443").await?;

                    let (sender, recv) = socket.split();

                    *locked_sender = sender;

                    receiver = recv;

                    login(locked_sender, api_info).await?;
                } else if msg.contains("PING") {
                    ws_sender
                        .lock()
                        .await
                        .send(Message::Text(String::from("PONG :tmi.twitch.tv")))
                        .await?;
                }
            }
            Ok(_) => {}
            Err(e) => {
                println!("IRC::read:: {e}");
            }
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
