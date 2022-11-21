use std::sync::{Arc, Mutex};

use eyre::WrapErr;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use libmpv::Mpv;
use sadmadbotlad::flatten;
use tokio::{
    net::TcpStream,
    sync::mpsc::{self, Sender},
};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::song_requests::{play_song, Queue, SongRequest, SongRequestSetup};

pub async fn irc_connect() -> eyre::Result<()> {
    let (socket, _) = connect_async("wss://irc-ws.chat.twitch.tv:443").await?;

    let (ws_sender, ws_receiver) = socket.split();

    login(ws_sender).await?;

    let (sender, receiver) = mpsc::channel::<SongRequest>(200);

    let sr_setup = SongRequestSetup::new();

    let queue = sr_setup.queue.clone();

    let mpv = sr_setup.mpv.clone();

    let join_handle = std::thread::spawn(move || play_song(receiver, mpv, queue));

    if let Err(e) = tokio::try_join!(flatten(tokio::spawn(async move {
        read(sender, ws_receiver, sr_setup.mpv, sr_setup.queue).await
    })))
    .wrap_err_with(|| "songs")
    {
        eprintln!("{e}")
    }

    join_handle.join().expect("thread join");

    Ok(())
}

async fn login(
    mut sender: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
) -> Result<(), eyre::Report> {
    let nick_msg = Message::Text(String::from("NICK justinfan5005"));

    sender.send(nick_msg).await?;

    sender
        .send(Message::Text(String::from("JOIN #sadmadladsalman")))
        .await?;

    Ok(())
}

async fn read(
    sender: Sender<SongRequest>,
    mut receiver: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    mpv: Arc<Mpv>,
    queue: Arc<Mutex<Queue>>,
) -> eyre::Result<()> {
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(msg)) if msg.contains("PRIVMSG") => {
                let parsed_msg = parse_message(&msg);

                let parsed_sender = parse_sender(&msg);

                println!("{}: {}", parsed_sender, parsed_msg);

                if parsed_msg.starts_with("!sr ") {
                    let song_url = parsed_msg.split(' ').collect::<Vec<&str>>()[1];

                    let song = SongRequest {
                        user: parsed_sender,
                        song: song_url.to_string(),
                    };

                    sender.send(song.clone()).await.expect("send song");

                    queue
                        .lock()
                        .expect("queue lock")
                        .enqueue(song)
                        .expect("Enqueuing");
                } else if parsed_msg.starts_with("!skip") {
                    if let Err(e) = mpv.playlist_next_force() {
                        println!("{e}");
                    }
                } else if parsed_msg.starts_with("!volume ") {
                    let Ok(value) = parsed_msg.split(' ').collect::<Vec<&str>>()[1].parse::<i64>() else {
                        continue;
                    };

                    if let Err(e) = mpv.set_property("volume", value) {
                        println!("{e}");
                    }
                } else if parsed_msg.starts_with("!pause") {
                    if let Err(e) = mpv.pause() {
                        println!("{e}");
                    }
                } else if parsed_msg.starts_with("!play") {
                    if let Err(e) = mpv.unpause() {
                        println!("{e}");
                    }
                } else if parsed_msg.starts_with("!queue") {
                    println!("{:#?}", queue.lock().expect("queue"));
                } else if parsed_msg.starts_with("!currentsong") {
                    println!("{:#?}", queue.lock().expect("queue").current_song);
                }
                // do this when you do permissions and shit
                // else if parsed_msg.starts_with("!title ") {

                // }
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
