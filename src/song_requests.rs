use std::sync::{Arc, Mutex};

use libmpv::events::{Event, PropertyData};
use libmpv::{FileState, Mpv};
use tokio::sync::mpsc::{Receiver, Sender};

pub struct SongRequestSetup {
    pub queue: Arc<Mutex<Queue>>,
    pub mpv: Arc<Mpv>,
    sender: Sender<SongRequest>,
    receiver: Receiver<SongRequest>,
}

impl SongRequestSetup {
    pub fn new() -> Self {
        let (sender, receiver) = tokio::sync::mpsc::channel(200);

        Self {
            queue: Arc::new(Mutex::new(Queue::new())),
            mpv: Arc::new(setup_mpv()),
            sender,
            receiver,
        }
    }

    async fn sr(&mut self, irc_msg: &str, irc_sender: String) -> String {
        // request is a video title
        if !irc_msg.starts_with("https://") {
            let video_info = youtube::video_info(irc_msg, api_info);

            let song = SongRequest {
                title: video_info.await.title,
                user: irc_sender,
                url: format!("https://www.youtube.com/watch/{}", video_info.id),
            };

            sender.send(song.clone()).await.expect("send song");

            queue
                .lock()
                .expect("read:: queue lock")
                .enqueue(song)
                .expect("Enqueuing");

            return format!("PRIVMSG #sadmadladsalman :Added: {}", song.title);
        }
        // request is a valid yt URL

        let Ok(video_id) = youtube::video_id_from_url(irc_msg) else {
        eprintln!("not a valid url");
        return;
    };

        let video_title = youtube::video_title(irc_msg);

        let song = SongRequest {
            title: video_title,
            user: parsed_sender,
            url: format!("https://youtube.com/watch/{}", video_id),
        };

        sender.send(song.clone()).await.expect("send song");

        queue
            .lock()
            .expect("read:: queue lock")
            .enqueue(song)
            .expect("Enqueuing");

        format!("PRIVMSG #sadmadladsalman :Added: {}", video_title)
    }
}

#[derive(Default, Debug, Clone)]
pub struct SongRequest {
    pub title: String,
    pub url: String,
    pub user: String,
}

#[derive(Default, Debug, Clone)]
pub struct Queue {
    pub current_song: Option<SongRequest>,
    pub queue: [Option<SongRequest>; 20],
    pub rear: usize,
}

impl Queue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue(&mut self, item: SongRequest) -> Result<(), &str> {
        if self.rear >= 20 {
            return Err("queue is full");
        }

        self.queue[self.rear] = Some(item);

        self.rear += 1;

        Ok(())
    }

    pub fn dequeue(&mut self) -> Result<(), &'static str> {
        self.current_song = self.queue[0].clone();

        for i in 0..self.rear - 1 {
            self.queue[i] = self.queue[i + 1].clone();
            self.queue[i + 1] = None;
        }

        if self.rear == 1 {
            self.queue[0] = None;
        }

        self.rear -= 1;

        Ok(())
    }
}

pub fn setup_mpv() -> Mpv {
    let Ok(mpv) = Mpv::new() else {
        panic!("mpv crashed")
    };

    mpv.set_property("volume", 20).expect("mpv volume");

    mpv.set_property("video", "no").expect("mpv no video");

    mpv
}

pub fn play_song(mut receiver: Receiver<SongRequest>, mpv: Arc<Mpv>, queue: Arc<Mutex<Queue>>) {
    let mut event_ctx = mpv.create_event_context();

    event_ctx
        .disable_deprecated_events()
        .expect("deprecated events");

    event_ctx
        .observe_property("idle-active", libmpv::Format::Flag, 0)
        .expect("observe property");

    loop {
        let ev = event_ctx
            .wait_event(600.)
            .unwrap_or(Err(libmpv::Error::Null));

        match ev {
            Ok(Event::PropertyChange {
                name: "idle-active",
                change: PropertyData::Flag(true),
                ..
            }) => {
                if let Some(song) = receiver.blocking_recv() {
                    mpv.playlist_load_files(&[(&song.url, FileState::AppendPlay, None)])
                        .expect("play song");

                    let mut locked_queue = queue.lock().expect("queue lock");

                    if locked_queue.rear == 0 {
                        locked_queue.current_song = None;
                    }

                    if let Err(e) = locked_queue.dequeue() {
                        println!("play_song:: {e}");
                    }

                    println!("{song:?}");
                }
            }
            Err(libmpv::Error::Raw(e)) => {
                queue.lock().expect("queue lock").current_song = None;

                println!("{e}");

                // let e_str = e.to_string();

                // let e = &e_str[e_str.find('(').unwrap() + 1..e_str.chars().count() - 1];

                // println!(
                //     "MpvEvent:: {e}:{}",
                //     libmpv_sys::mpv_error_str(e.parse::<i32>().unwrap())
                // );
            }
            _ => {}
        }
    }
}
