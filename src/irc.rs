use std::sync::{Arc, Mutex};

use eyre::WrapErr;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use libmpv::Mpv;
use sadmadbotlad::{flatten, ApiInfo};
use tokio::{
    net::TcpStream,
    sync::{mpsc::{self, Sender}, MutexGuard},
};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::song_requests::{play_song, Queue, SongRequest, SongRequestSetup};

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

    if let Err(e) = tokio::try_join!(flatten(tokio::spawn(async move {
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
    .wrap_err_with(|| "songs")
    {
        eprintln!("{e}")
    }

    join_handle.join().expect("thread join");

    Ok(())
}

async fn login<'a>(
    mut locked_sender: MutexGuard<'a, SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>,
    api_info: &ApiInfo,
) -> Result<(), eyre::Report> {

    let pass_msg = Message::Text(format!("PASS oauth:{}", api_info.twitch_oauth));

    locked_sender.send(pass_msg).await?;

    let nick_msg = Message::Text(String::from("NICK sadmadbotlad"));

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
    println!("a");

    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(msg))
                if msg.contains("PRIVMSG") && parse_message(&msg).starts_with("!") =>
            {
                let mut locked_sender = ws_sender.lock().await;

                let parsed_msg = parse_message(&msg);

                let parsed_sender = parse_sender(&msg);

                println!("{}: {}", parsed_sender, parsed_msg);

                if parsed_msg.starts_with("!ping") {

                    locked_sender
                        .send(Message::Text(String::from("PRIVMSG #sadmadladsalman :!pong")))
                        .await
                        .expect("read::sr: chat");
                    
                    println!("!pong");

                } else if parsed_msg.starts_with("!sr ") {
                    let song_url = parsed_msg.split(' ').collect::<Vec<&str>>()[1];

                    let song = SongRequest {
                        user: parsed_sender,
                        song_url: song_url.to_string(),
                    };

                    sender.send(song.clone()).await.expect("send song");

                    queue
                        .lock()
                        .expect("read:: queue lock")
                        .enqueue(song.clone())
                        .expect("Enqueuing");

                    locked_sender
                        .send(Message::Text(format!(
                            "PRIVMSG #sadmadladsalman :added {}",
                            song.song_url
                        )))
                        .await
                        .expect("read::sr: chat");
                } else if parsed_msg.starts_with("!skip") {
                    if let Err(e) = mpv.playlist_next_force() {
                        println!("{e}");
                    }

                    locked_sender
                        .send(Message::Text(format!(
                            "PRIVMSG #sadmadladsalman :skipped {}",
                            queue
                                .lock()
                                .expect("read:: queue")
                                .current_song
                                .as_ref()
                                .expect("read::!skip: current song")
                                .song_url
                        )))
                        .await
                        .expect("read::sr: chat");

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

                    locked_sender
                        .send(Message::Text(format!(
                            "PRIVMSG #sadmadladsalman :volume set to {value}",
                        )))
                        .await
                        .expect("read::sr: chat");
                } else if parsed_msg.starts_with("!volume") {
                    let Ok(volume) = mpv.get_property::<i64>("volume") else {
                        panic!("read:: volume");
                    };

                    locked_sender
                        .send(Message::Text(format!(
                            "PRIVMSG #sadmadladsalman :volume: {}",
                            volume
                        )))
                        .await
                        .expect("read::sr: chat");
                } else if parsed_msg.starts_with("!pause") {
                    if let Err(e) = mpv.pause() {
                        println!("{e}");
                    }

                    locked_sender
                        .send(Message::Text(format!("PRIVMSG #sadmadladsalman :Paused",)))
                        .await
                        .expect("read::sr: chat");
                } else if parsed_msg.starts_with("!play") {
                    if let Err(e) = mpv.unpause() {
                        println!("{e}");
                    }

                    locked_sender
                        .send(Message::Text(format!(
                            "PRIVMSG #sadmadladsalman :Resumed {}",
                            queue
                                .lock()
                                .expect("read::!play: queue lock")
                                .current_song
                                .as_ref()
                                .expect("read::!play: current song")
                                .song_url
                        )))
                        .await
                        .expect("read::sr: chat");
                } else if parsed_msg.starts_with("!queue") {
                    println!("{:#?}", queue.lock().expect("read:: queue"));

                    locked_sender
                        .send(Message::Text(format!(
                            "PRIVMSG #sadmadladsalman :Queue: {:#?}",
                            queue.lock().expect("read::!queue: queue lock")
                        )))
                        .await
                        .expect("read::sr: chat");
                } else if parsed_msg.starts_with("!currentsong") {
                    println!("{:#?}", queue.lock().expect("read:: queue").current_song);

                    locked_sender
                        .send(Message::Text(format!(
                            "PRIVMSG #sadmadladsalman :Current song: {:#?}",
                            queue.lock().expect("read:: queue").current_song
                        )))
                        .await
                        .expect("read::sr: chat");
                }
                // do this when you do permissions and shit
                // else if parsed_msg.starts_with("!title ") {

                // }
            }
            Ok(Message::Text(msg)) if msg.contains("RECONNECT") => {
                println!("Reconnecting IRC");
                
                let mut locked_sender = ws_sender.lock().await;
                
                let (socket, _) = connect_async("wss://irc-ws.chat.twitch.tv:443").await?;
                
                let (sender, recv) = socket.split();
                
                *locked_sender = sender;
                
                receiver = recv;
                
                login(locked_sender, api_info).await?;
            }
            Ok(_) => {}
            Err(e) => println!("{e}"),
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
