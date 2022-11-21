use eyre::WrapErr;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use sadmadbotlad::flatten;
use tokio::{net::TcpStream, sync::mpsc::{self, Sender}};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::song_requests::{SongRequest, play_song};

pub async fn irc_connect() -> eyre::Result<()> {
    let (socket, _) = connect_async("wss://irc-ws.chat.twitch.tv:443").await?;

    let (ws_sender, ws_receiver) = socket.split();

    login(ws_sender).await?;

    let (sender, receiver) = mpsc::channel::<SongRequest>(200);
    
    let join_handle = std::thread::spawn(move || {
        play_song(receiver)
    });
    
    if let Err(e) = tokio::try_join!(
        flatten(tokio::spawn(async move {
            read(sender, ws_receiver).await
        }))
    )
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

                    sender.send(song).await.expect("send song");
                }
                
            }
            Ok(_) => {}
            Err(e) => println!("{e}"),
        }
    }

    Ok(())
}

fn parse_message(msg: &str) -> String {
    msg[(&msg[1..]).find(':').unwrap() + 2..]
        .replace("\r\n", "")
        .to_string()
}

fn parse_sender(msg: &str) -> String {
    msg[1..msg.find('!').unwrap()].to_string()
}
