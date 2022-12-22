use std::sync::Arc;

use libmpv::events::{Event, PropertyData};
use libmpv::{FileState, Mpv};
use serde::{Serialize, Deserialize};
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};

use crate::irc::to_irc_message;
use crate::{youtube, ApiInfo};
// use tokio::sync::broadcast::Sender;
// // use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};

// use crate::youtube;
use html_escape::decode_html_entities;

pub struct SongRequestSetup {
    pub queue: Queue,
    pub api_info: ApiInfo,
}

impl SongRequestSetup {
    pub fn new() -> Result<Self, eyre::Report> {
        Ok(Self {
            queue: Queue::new(),
            api_info: ApiInfo::new()?,
        })
    }

    pub async fn sr(
        &mut self,
        irc_msg: &str,
        irc_sender: impl Into<String>,
        song_sender: &Sender<SongRequest>,
    ) -> Result<String, eyre::Report> {
        // request is a video title
        if !irc_msg.starts_with("https://") {
            let video_info = youtube::video_info(irc_msg, &self.api_info)
                .await?;

            let song = SongRequest {
                title: video_info.title,
                user: irc_sender.into(),
                url: format!("https://www.youtube.com/watch/{}", video_info.id),
                id: video_info.id,
            };

            song_sender.send(song.clone()).await.expect("send song");

            self.queue.enqueue(&song).expect("Enqueuing");

            return Ok(to_irc_message(format!("Added: {}", song.title)));
        }
        // request is a valid yt URL

        let video_id = youtube::video_id_from_url(irc_msg)?;

        let video_title = youtube::video_title(irc_msg, &self.api_info)
            .await?;

        let video_title = decode_html_entities(&video_title).to_string();
        
        let song = SongRequest {
            title: video_title.clone(),
            user: irc_sender.into(),
            url: format!("https://youtube.com/watch/{}", video_id),
            id: video_id.to_string(),
        };

        song_sender.send(song.clone()).await.expect("send song");

        self.queue.enqueue(&song).expect("Enqueuing");

        return Ok(to_irc_message(format!("Added: {}", video_title)));
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct SongRequest {
    pub title: String,
    pub url: String,
    pub id: String,
    pub user: String,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
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

    pub fn dequeue(&mut self) {
        if self.rear == 0 {
            self.current_song = None;
            return;
        }

        self.current_song = self.queue[0].clone();

        for i in 0..self.rear - 1 {
            self.queue[i] = self.queue[i + 1].clone();
            self.queue[i + 1] = None;
        }

        if self.rear == 1 {
            self.queue[0] = None;
        }

        self.rear -= 1;
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
    mpv: Arc<Mpv>,
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
                event_sender.send(crate::event_handler::Event::MpvEvent(
                    crate::event_handler::MpvEvent::DequeueSong,
                ))?;

                if let Some(song) = song_receiver.blocking_recv() {
                    println!("{song:#?}");

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
