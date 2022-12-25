use serde::{Deserialize, Serialize};

pub mod alerts;
pub mod songs;
pub mod components;
pub mod activity_feed;

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

pub enum Msg {
    Follow(String),
    Raid(String),
    Clear(()),
    RequestSongs,
    SongsResponse(String),
    Nothing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertEventType {
    Follow {
        follower: String,
    },
    Raid {
        from: String,
    },
    Subscribe {
        subscriber: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Alert {
    new: bool,
    alert_type: AlertEventType,
}

