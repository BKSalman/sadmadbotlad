use std::sync::Arc;

use crate::{
    event_handler::{self, event_handler, Event, IrcChat, IrcEvent, IrcWs},
    flatten,
    song_requests::{play_song, setup_mpv, SongRequest},
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

    let (e_sender, e_receiver) = tokio::sync::mpsc::unbounded_channel::<event_handler::Event>();

    let (song_sender, song_receiver) = tokio::sync::mpsc::channel::<SongRequest>(200);

    let e_sender_c = e_sender.clone();

    let mpv = Arc::new(setup_mpv());

    let join_handle =
        std::thread::spawn(move || play_song(mpv.as_ref(), song_receiver, e_sender_c));

    tokio::try_join!(
        flatten(tokio::spawn(read(e_sender, ws_receiver))),
        flatten(tokio::spawn(event_handler(song_sender, e_receiver, ws_sender))),
    )
    .wrap_err_with(|| "songs")?;

    join_handle.join().expect("join")?;

    Ok(())
}

pub async fn irc_login<'a>(
    ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    api_info: &ApiInfo,
) -> Result<(), eyre::Report> {
    let pass_msg = Message::Text(format!("PASS oauth:{}", api_info.twitch_oauth));

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
    mut ws_receiver: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
) -> eyre::Result<()> {
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(Message::Text(msg)) if msg.contains("PRIVMSG") => {
                let parsed_sender = parse_sender(&msg);
                let parsed_msg = parse_message(&msg);
                
                if parsed_msg.starts_with("!ping") {
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Ping)))?;
                } else if parsed_msg.starts_with("!sr ") {
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Sr((parsed_sender, parsed_msg)))))?;
                } else if parsed_msg.starts_with("!sr") {
                    let e = String::from("PRIVMSG #sadmadladsalman :Correct usage: !sr <Link>");
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::Invalid(e))))?;
                } else if parsed_msg.starts_with("!skip") {
                    event_sender.send(Event::IrcEvent(IrcEvent::Chat(IrcChat::SkipSr)))?;
                }
            }
            Ok(Message::Text(msg)) => {
                if msg.contains("PING") {
                    event_sender.send(Event::IrcEvent(IrcEvent::WebSocket(IrcWs::Ping)))?;
                }
            }
            Ok(_) => {}
            Err(e) => println!("{e}"),
        }
    }

    Ok(())
}
// async fn read(
//     mut event_sender: UnboundedSender<Event>,
//     mut ws_receiver: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
// ) -> eyre::Result<()> {
//     while let Some(msg) = ws_receiver.next().await {
//         match msg {
//             Ok(Message::Text(msg)) => {
//                 if msg.contains("PRIVMSG") {
//                     let parsed_msg = parse_message(&msg);

//                     if !parsed_msg.starts_with('!') {
//                         continue;
//                     }

//                     let parsed_sender = parse_sender(&msg);

//                     println!("{}: {}", parsed_sender, parsed_msg);

//                     let mut message = String::new();

//                     if parsed_msg.starts_with("!ping") {
//                         // abstract this to a function that sends messages to the chat
//                         message = String::from("PRIVMSG #sadmadladsalman :!pong");

//                         println!("!pong");
//                     } else if parsed_msg.starts_with("!title ")
//                         && parsed_sender.to_ascii_lowercase() == "sadmadladsalman"
//                     {
//                         twitch::change_title(&parsed_msg, &sr_setup.read().unwrap().api_info)
//                             .await?;
//                         message =
//                             format!("PRIVMSG #sadmadladsalman :Changed title to {}", parsed_msg);
//                     } else if parsed_msg.starts_with("!title") {
//                         message = format!(
//                             "PRIVMSG #sadmadladsalman :Title: {}",
//                             twitch::get_title(&sr_setup.as_ref().lock().unwrap().api_info).await?
//                         );
//                     } else if parsed_msg.starts_with("!sr ") {
//                         // discard "!sr " and get the requested song
//                         let sr_msg = parsed_msg.splitn(2, ' ').collect::<Vec<&str>>()[1];
//                         message = sr_setup
//                             .as_ref()
//                             .lock()
//                             .unwrap()
//                             .sr(sr_msg, parsed_sender)
//                             .await?;
//                     } else if parsed_msg.starts_with("!skip") {
//                         if let Err(e) = mpv.playlist_next_force() {
//                             println!("{e}");
//                         }

//                         let video_title = message = format!(
//                             "PRIVMSG #sadmadladsalman :Skipped {}",
//                             queue
//                                 .lock()
//                                 .expect("read:: queue")
//                                 .current_song
//                                 .as_ref()
//                                 .expect("read::!skip: current song")
//                                 .title
//                         );

//                         if queue.lock().expect("read:: queue").rear == 0 {
//                             queue.lock().expect("read:: queue").current_song = None;
//                         }
//                     } else if parsed_msg.starts_with("!volume ") {
//                         let Ok(value) = parsed_msg.split(' ').collect::<Vec<&str>>()[1].parse::<i64>() else {
//                             continue;
//                         };

//                         if let Err(e) = mpv.set_property("volume", value) {
//                             println!("{e}");
//                         }

//                         message = format!("PRIVMSG #sadmadladsalman :Volume set to {value}",);
//                     } else if parsed_msg.starts_with("!volume") {
//                         let Ok(volume) = mpv.get_property::<i64>("volume") else {
//                             panic!("read:: volume");
//                         };

//                         message = format!("PRIVMSG #sadmadladsalman :Volume: {}", volume);
//                     } else if parsed_msg.starts_with("!pause") {
//                         if let Err(e) = mpv.pause() {
//                             println!("{e}");
//                         }

//                         message = String::from("PRIVMSG #sadmadladsalman :Paused");
//                     } else if parsed_msg.starts_with("!play") {
//                         if let Some(something) =
//                             &queue.lock().expect("read::!play: queue lock").current_song
//                         {
//                             if let Err(e) = mpv.unpause() {
//                                 println!("{e}");
//                             }

//                             message = format!("PRIVMSG #sadmadladsalman :Resumed {:?}", something);
//                         } else {
//                             message = String::from("PRIVMSG #sadmadladsalman :Queue is empty");
//                         }
//                     } else if parsed_msg.starts_with("!queue") {
//                         let locked_queue = queue.lock().expect("read::!queue: queue lock");

//                         let mut queue = String::new();

//                         for (s, i) in locked_queue.clone().queue.iter().zip(1..=20) {
//                             if let Some(song) = s {
//                                 queue.push_str(&format!(" {i}- {} :: by {}", song.url, song.user,));
//                             }
//                         }

//                         println!("{:#?}", locked_queue);

//                         if queue.chars().count() <= 0 {
//                             message = format!("PRIVMSG #sadmadladsalman :Queue is empty");
//                         } else {
//                             message = format!("PRIVMSG #sadmadladsalman :Queue:{}", queue);
//                         }
//                     } else if parsed_msg.starts_with("!currentsong") {
//                         let locked_queue = queue.lock().expect("read:: queue");

//                         println!("{locked_queue:#?}");

//                         if let Some(current_song) = locked_queue.current_song.as_ref() {
//                             message = format!(
//                                 "PRIVMSG #sadmadladsalman :Current song: {} - by {}",
//                                 current_song.title, current_song.user,
//                             );
//                         } else {
//                             message = String::from("PRIVMSG #sadmadladsalman :No song playing");
//                         }
//                     }

//                     let mut locked_sender = ws_sender.lock().await;

//                     locked_sender
//                         .send(Message::Text(message))
//                         .await
//                         .expect("read::sr: chat");
//                 } else if msg.contains("RECONNECT") {
//                     println!("Reconnecting IRC");

//                     let mut locked_sender = ws_sender.lock().await;

//                     let (socket, _) = connect_async("wss://irc-ws.chat.twitch.tv:443").await?;

//                     let (sender, recv) = socket.split();

//                     *locked_sender = sender;

//                     ws_receiver = recv;

//                     login(locked_sender, api_info).await?;
//                 } else if msg.contains("PING") {
//                     ws_sender
//                         .lock()
//                         .await
//                         .send(Message::Text(String::from("PONG :tmi.twitch.tv")))
//                         .await?;
//                 }
//             }
//             Ok(_) => {}
//             Err(e) => {
//                 println!("IRC::read:: {e}");
//             }
//         }
//     }

//     Ok(())
// }

fn parse_message(msg: &str) -> String {
    msg.trim()[(&msg[1..]).find(':').unwrap() + 2..]
        .replace("\r\n", "")
        .to_string()
}

fn parse_sender(msg: &str) -> String {
    msg.trim()[1..msg.find('!').unwrap()].to_string()
}
