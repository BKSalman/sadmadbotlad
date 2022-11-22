use std::sync::{Arc, Mutex};

use libmpv::events::{Event, PropertyData};
use libmpv::{FileState, Mpv};
use tokio::sync::mpsc::Receiver;

pub struct SongRequestSetup {
    pub queue: Arc<Mutex<Queue>>,
    pub mpv: Arc<Mpv>,
}

impl SongRequestSetup {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(Queue::new())),
            mpv: Arc::new(setup_mpv()),
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct SongRequest {
    pub song_url: String,
    pub user: String,
}

impl SongRequest {
    pub fn empty() -> Self {
        Self {
            song_url: String::new(),
            user: String::new(),
        }
    }
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
                    mpv.playlist_load_files(&[(&song.song_url, FileState::AppendPlay, None)])
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

                let e_str = e.to_string();

                let e = &e_str[e_str.find('(').unwrap() + 1..e_str.chars().count() - 1];

                println!(
                    "MpvEvent:: {e}:{}",
                    libmpv_sys::mpv_error_str(e.parse::<i32>().unwrap())
                );
            }
            _ => {},
        }
    }
}
