use std::sync::{Arc, Condvar, Mutex};
use eyre::WrapErr;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use libmpv::Mpv;
use sadmadbotlad::flatten;
// use sadmadbotlad::flatten;
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

#[derive(Default, Debug, Clone)]
pub struct SongRequest {
    pub song: String,
    pub user: String,
}

#[derive(Default, Debug, Clone)]
struct Queue {
    queue: [Option<SongRequest>; 20],
    rear: usize,
}

impl Queue {
    fn new() -> Self {
        Self::default()
    }

    fn enqueue(&mut self, item: SongRequest) -> Result<(), &str> {
        if self.rear >= 20 {
            return Err("queue is full");
        }

        self.queue[self.rear] = Some(item);

        self.rear += 1;

        Ok(())
    }

    fn dequeue(&mut self) -> Result<Option<SongRequest>, &'static str> {
        if self.rear <= 0 {
            return Err("queue is empty");
        }

        let value = self.queue[self.rear].clone();

        for i in 0..self.rear - 1 {
            self.queue[i] = self.queue[i + 1].clone();
            self.queue[i + 1] = None;
        }

        self.rear -= 1;

        Ok(value)
    }
}

pub async fn irc_connect() -> eyre::Result<()> {
    let (socket, _) = connect_async("wss://irc-ws.chat.twitch.tv:443").await?;

    let (sender, receiver) = socket.split();

    login(sender).await?;

    let queue = Arc::new(Mutex::new(Queue::new()));

    let condvar = Arc::new(Condvar::new());

    if let Err(e) = tokio::try_join!(
        flatten(tokio::spawn(play_song(queue.clone(), condvar.clone()))),
        flatten(tokio::spawn(async move {
            read(queue.clone(), receiver, condvar).await
        }))
    )
    .wrap_err_with(|| "songs")
    {
        eprintln!("{e}")
    }

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
    queue: Arc<Mutex<Queue>>,
    mut receiver: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    condvar: Arc<Condvar>,
) -> eyre::Result<()> {
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(msg)) if msg.contains("PRIVMSG") => {
                let parsed_msg = parse_message(&msg);

                let parsed_sender = parse_sender(&msg);

                if parsed_msg.starts_with("!sr ") {
                    let song_url = parsed_msg.split(' ').collect::<Vec<&str>>()[1];

                    let song = SongRequest {
                        user: parsed_sender,
                        song: song_url.to_string(),
                    };

                    queue.lock().expect("queue lock").enqueue(song);
                    condvar.notify_one();
                }
            }
            Ok(_) => {}
            Err(e) => println!("{e}"),
        }
    }

    Ok(())
}

fn parse_message(msg: &String) -> String {
    msg[(&msg[1..]).find(':').unwrap() + 2..]
        .replace("\r\n", "")
        .to_string()
}

fn parse_sender(msg: &String) -> String {
    msg[1..msg.find('!').unwrap()].to_string()
}

async fn play_song(queue: Arc<Mutex<Queue>>, condvar: Arc<Condvar>) -> eyre::Result<()> {
    let queue = condvar.wait(queue.lock().unwrap()).unwrap();
    
    // mpv event when the song finishes to play the next one
    // maybe use another condvar for this
    
    let Ok(mpv) = Mpv::new() else {
        panic!("mpv crashed")
    };
    
    let Ok(()) = mpv.set_property("volume", 15) else {
        panic!("set property")
    };
    
    while let Some(Some(song)) = queue.queue.iter().next() {
        // run mpv thing
    }
    
    Ok(())
}
