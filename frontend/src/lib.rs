use serde::{Deserialize, Serialize};

pub mod alerts;
pub mod songs;
pub mod components;

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

