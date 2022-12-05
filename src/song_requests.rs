use libmpv::events::{Event, PropertyData};
use libmpv::{FileState, Mpv};
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};

use crate::irc::to_irc_msg;
use crate::{youtube, ApiInfo};
// use tokio::sync::broadcast::Sender;
// // use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};

// use crate::youtube;

pub struct SongRequestSetup {
    pub queue: Queue,
    pub mpv: Mpv,
    pub api_info: ApiInfo,
}

impl SongRequestSetup {
    pub fn new() -> Self {
        Self {
            queue: Queue::new(),
            mpv: setup_mpv(),
            api_info: ApiInfo::new(),
        }
    }

    pub async fn sr(
        &mut self,
        irc_msg: &str,
        irc_sender: String,
        song_sender: &Sender<SongRequest>,
    ) -> Result<String, std::io::Error> {
        // request is a video title
        if !irc_msg.starts_with("https://") {
            let video_info = youtube::video_info(irc_msg, &self.api_info)
                .await
                .map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::ConnectionRefused,
                        "couldn't get video info",
                    )
                })?;

            let song = SongRequest {
                title: video_info.title,
                user: irc_sender,
                url: format!("https://www.youtube.com/watch/{}", video_info.id),
            };

            song_sender.send(song.clone()).await.expect("send song");

            self.queue.enqueue(&song).expect("Enqueuing");

            return Ok(to_irc_msg(&format!("Added: {}", song.title), &self.api_info.user));
        }
        // request is a valid yt URL

        let Ok(video_id) = youtube::video_id_from_url(irc_msg) else {
                eprintln!("not a valid url");
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid URL"));
            };
        let video_title = youtube::video_title(irc_msg, &self.api_info)
            .await
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    "couldn't get video info",
                )
            })?;

        let song = SongRequest {
            title: video_title.clone(),
            user: irc_sender,
            url: format!("https://youtube.com/watch/{}", video_id),
        };

        song_sender.send(song.clone()).await.expect("send song");

        self.queue.enqueue(&song).expect("Enqueuing");

        return Ok(to_irc_msg(&format!("Added: {}", video_title), &self.api_info.user));
    }

    pub fn skip(&mut self) -> Result<String, eyre::Report> {
        if let Some(song) = self.queue.dequeue().expect("skip:: dequeue") {
            if let Err(e) = self.mpv.playlist_next_force() {
                println!("{e}");
            }
            let message = to_irc_msg(&format!("Skipped {}", song.title), &self.api_info.user);
            return Ok(message);
        } else {
            let message = to_irc_msg("Queue is empty", &self.api_info.user);
            return Ok(message);
        }
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

    pub fn enqueue(&mut self, item: &SongRequest) -> Result<(), &str> {
        if self.rear >= 20 {
            return Err("queue is full");
        }

        self.queue[self.rear] = Some(item.clone());

        self.rear += 1;

        Ok(())
    }

    pub fn dequeue(&mut self) -> Result<Option<SongRequest>, eyre::Report> {
        if self.rear == 0 {
            let current_song = self.current_song.clone();
            self.current_song = None;
            return Ok(current_song);
        }
        
        self.current_song = self.queue[0].clone();

        let dequeued_song = self.queue[0].clone();

        for i in 0..self.rear - 1 {
            self.queue[i] = self.queue[i + 1].clone();
            self.queue[i + 1] = None;
        }

        if self.rear == 1 {
            self.queue[0] = None;
        }

        self.rear -= 1;

        Ok(dequeued_song)
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

pub fn play_song(
    mpv: &Mpv,
    mut song_receiver: Receiver<SongRequest>,
    event_sender: UnboundedSender<crate::event_handler::Event>,
) -> Result<(), eyre::Report> {
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
                if let Some(song) = song_receiver.blocking_recv() {
                    mpv.playlist_load_files(&[(&song.url, FileState::AppendPlay, None)])
                        .expect("play song");

                    event_sender.send(crate::event_handler::Event::MpvEvent(
                        crate::event_handler::MpvEvent::DequeueSong,
                    ))?;
                }
            }
            Err(libmpv::Error::Raw(e)) => {
                event_sender.send(crate::event_handler::Event::MpvEvent(
                    crate::event_handler::MpvEvent::Error(e),
                ))?;
            }
            _ => {}
        }
    }
}
