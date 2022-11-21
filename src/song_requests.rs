use std::sync::{Arc, Mutex};

use libmpv::events::Event;
use libmpv::{FileState, Mpv};
use tokio::sync::mpsc::Receiver;

#[derive(Default, Debug, Clone)]
pub struct SongRequest {
    pub song: String,
    pub user: String,
}

#[derive(Default, Debug, Clone)]
pub struct Queue {
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

    pub fn dequeue(&mut self) -> Result<Option<SongRequest>, &'static str> {
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

        if let Ok(song) = queue.lock().expect("queue lock").dequeue() {
            println!("{song:?}");
        }
    }

    loop {
        let ev = event_ctx
            .wait_event(120.)
            .unwrap_or(Err(libmpv::Error::Null));

        if let Some(song) = receiver.blocking_recv() {
            mpv.playlist_load_files(&[(&song.song, FileState::AppendPlay, None)])
                .expect("play song");

            if let Ok(song) = queue.lock().expect("queue lock").dequeue() {
                println!("{song:?}");
            }
        }

        match ev {
            Ok(Event::EndFile(_)) => {
                if let Some(something) = receiver.blocking_recv() {
                    mpv.playlist_load_files(&[(&something.song, FileState::AppendPlay, None)])
                        .expect("play song");

                    if let Ok(song) = queue.lock().expect("queue lock").dequeue() {
                        println!("{song:?}");
                    }
                }
            }
            Ok(_) => {}
            Err(e) => println!("{e}"),
        }
    }
}
