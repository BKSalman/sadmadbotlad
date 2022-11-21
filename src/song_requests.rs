use std::sync::{Arc, Mutex};

use libmpv::events::Event;
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
    pub song: String,
    pub user: String,
}

#[derive(Default, Debug, Clone)]
pub struct Queue {
    pub current_song: Option<SongRequest>,
    queue: [Option<SongRequest>; 20],
    rear: usize,
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

    mpv.set_property("volume", 25).expect("mpv volume");

    mpv.set_property("video", "no").expect("mpv no video");

    mpv
}

pub fn play_song(mut receiver: Receiver<SongRequest>, mpv: Arc<Mpv>, queue: Arc<Mutex<Queue>>) {
    let mut event_ctx = mpv.create_event_context();

    event_ctx
        .disable_deprecated_events()
        .expect("deprecated events");

        if let Some(song) = receiver.blocking_recv() {
            mpv.playlist_load_files(&[(&song.song, FileState::AppendPlay, None)])
                .expect("play song");

            if let Err(e) = queue.lock().expect("queue lock").dequeue() {
                println!("play_song:: {e}");
            }

            println!("{song:?}");
        }

    loop {
        let ev = event_ctx
            .wait_event(120.)
            .unwrap_or(Err(libmpv::Error::Null));

        match ev {
            Ok(Event::EndFile(_)) => {
                if let Some(song) = receiver.blocking_recv() {
                    mpv.playlist_load_files(&[(&song.song, FileState::AppendPlay, None)])
                        .expect("play song");

                    if let Err(e) = queue.lock().expect("queue lock").dequeue() {
                        println!("play_song:: {e}");
                    }

                    println!("{song:?}");
                }
            }
            Ok(_) => {}
            Err(e) => println!("{e}"),
        }
    }
}
