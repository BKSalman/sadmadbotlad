use serde::{Deserialize, Serialize};

pub mod activity_feed;
pub mod alerts;
pub mod components;
pub mod songs;

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
        viewers: u64,
    },
    Subscribe {
        subscriber: String,
        tier: String,
    },
    ReSubscribe {
        subscriber: String,
        tier: String,
        subscribed_for: String,
        streak: u64,
    },
    GiftSub {
        gifter: String,
        total: u64,
        tier: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Alert {
    new: bool,
    alert_type: AlertEventType,
}
