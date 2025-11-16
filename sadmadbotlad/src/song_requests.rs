use std::sync::Arc;

use libmpv::events::{Event, PropertyData};
use libmpv::{FileState, Mpv};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, Receiver, Sender, UnboundedSender};
use tokio::sync::oneshot;

use crate::{ApiInfo, youtube};
use html_escape::decode_html_entities;

#[derive(thiserror::Error, Debug)]
pub enum SongRequestsError {
    #[error(transparent)]
    QueueMessagesSendError(#[from] mpsc::error::SendError<QueueMessages>),

    #[error(transparent)]
    OneShotSendError(#[from] oneshot::error::RecvError),

    #[error("could not get current song")]
    CouldNotGetCurrentSong,

    #[error("could not get mpv instance")]
    CouldNotGetMpv,
}

#[derive(Debug)]
pub enum QueueMessages {
    GetQueue(oneshot::Sender<Queue>),
    GetCurrentSong(oneshot::Sender<Option<SongRequest>>),
    Enqueue(SongRequest),
    Dequeue,
    ClearCurrentSong,
    Sr((String, String), oneshot::Sender<String>),
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct SongRequest {
    pub title: String,
    pub user: String,
    pub url: String,
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Queue {
    pub current_song: Option<SongRequest>,
    pub queue: [Option<SongRequest>; 20],
    pub rear: usize,
}

impl Queue {
    pub fn enqueue(&mut self, item: &SongRequest) -> anyhow::Result<()> {
        if self.rear >= 20 {
            return Err(anyhow::anyhow!("queue is full"));
        }

        self.queue[self.rear] = Some(item.clone());

        self.rear += 1;

        Ok(())
    }

    pub fn dequeue(&mut self) {
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

    pub fn clear_current_song(&mut self) {
        if self.rear == 0 {
            self.current_song = None;
        }
    }

    pub async fn sr(
        &mut self,
        sender: &str,
        song: &str,
        song_sender: Sender<SongRequest>,
        api_info: Arc<ApiInfo>,
    ) -> anyhow::Result<String> {
        // request is a video title
        if !song.starts_with("https://") {
            let video_info = youtube::video_info(song, api_info).await?;

            let song = SongRequest {
                title: video_info.title,
                user: sender.into(),
                url: format!("https://www.youtube.com/watch/{}", video_info.id),
                id: video_info.id,
            };

            song_sender.send(song.clone()).await.expect("send song");

            self.enqueue(&song).expect("Enqueuing");

            return Ok(format!("Added: {}", song.title));
        }
        // request is a valid yt URL

        let video_id = youtube::video_id_from_url(song)?;

        let video_title = youtube::video_title(song, api_info).await?;

        let video_title = decode_html_entities(&video_title).to_string();

        let song = SongRequest {
            title: video_title.clone(),
            user: sender.into(),
            url: format!("https://youtube.com/watch/{}", video_id),
            id: video_id.to_string(),
        };

        song_sender.send(song.clone()).await.expect("send song");

        self.enqueue(&song).expect("Enqueuing");

        Ok(format!("Added: {}", video_title))
    }
}

pub struct SrQueue {
    queue: Queue,
    api_info: Arc<ApiInfo>,
    song_sender: Sender<SongRequest>,
    receiver: mpsc::UnboundedReceiver<QueueMessages>,
}

impl SrQueue {
    pub fn new(
        api_info: Arc<ApiInfo>,
        song_sender: Sender<SongRequest>,
        receiver: mpsc::UnboundedReceiver<QueueMessages>,
    ) -> Self {
        Self {
            api_info,
            song_sender,
            receiver,
            queue: Queue {
                current_song: None,
                queue: Default::default(),
                rear: 0,
            },
        }
    }

    pub fn enqueue(&mut self, item: &SongRequest) -> anyhow::Result<()> {
        self.queue.enqueue(item)
    }

    pub fn dequeue(&mut self) {
        self.queue.dequeue();
    }

    pub fn clear_current_song(&mut self) {
        self.queue.clear_current_song();
    }

    pub async fn sr(
        &mut self,
        sender: &str,
        song: &str,
        song_sender: Sender<SongRequest>,
        api_info: Arc<ApiInfo>,
    ) -> anyhow::Result<String> {
        self.queue.sr(sender, song, song_sender, api_info).await
    }

    pub async fn handle_messages(mut self) -> anyhow::Result<()> {
        while let Some(message) = self.receiver.recv().await {
            match message {
                QueueMessages::GetQueue(one_shot_sender) => {
                    one_shot_sender
                        .send(self.queue.clone())
                        .expect("send queue");
                }
                QueueMessages::Enqueue(song) => self.enqueue(&song)?,
                QueueMessages::Dequeue => self.dequeue(),
                QueueMessages::ClearCurrentSong => self.clear_current_song(),
                QueueMessages::Sr((sender, song), one_shot_sender) => {
                    match self
                        .sr(
                            &sender,
                            &song,
                            self.song_sender.clone(),
                            self.api_info.clone(),
                        )
                        .await
                    {
                        Ok(message) => {
                            one_shot_sender.send(message).expect("send sr message");
                        }
                        Err(_) => {
                            // match e.to_string().as_str() {
                            //     "Not A Valid Youtube URL" => {
                            //         one_shot_sender
                            //             .send(String::from("Not a valid youtube URL"))
                            //             .expect("send invalid URL");
                            //     }
                            //     _ => panic!("{e}"),
                            // }
                            one_shot_sender
                                .send(String::from("Not a valid youtube URL"))
                                .expect("send invalid URL");
                        }
                    }
                }
                QueueMessages::GetCurrentSong(one_shot_sender) => {
                    one_shot_sender
                        .send(self.queue.current_song.clone())
                        .expect("send current song");
                }
            }
        }
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

pub fn play_song(
    mpv: Arc<Mpv>,
    mut song_receiver: Receiver<SongRequest>,
    queue_sender: UnboundedSender<QueueMessages>,
    // event_sender: UnboundedSender<crate::event_handler::Event>,
) -> Result<(), SongRequestsError> {
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
                queue_sender.send(QueueMessages::ClearCurrentSong)?;

                if let Some(song) = song_receiver.blocking_recv() {
                    tracing::info!("song: {song:#?}");

                    mpv.playlist_load_files(&[(&song.url, FileState::AppendPlay, None)])
                        .expect("play song");

                    queue_sender.send(QueueMessages::Dequeue)?;
                }
            }
            Err(libmpv::Error::Raw(e)) => {
                tracing::error!("Mpv Error:: {e}");
                queue_sender.send(QueueMessages::ClearCurrentSong)?;
                // event_sender.send(crate::event_handler::Event::MpvEvent(
                //     crate::event_handler::MpvEvent::Error(e),
                // ))?;
            }
            _ => {}
        }
    }
}
