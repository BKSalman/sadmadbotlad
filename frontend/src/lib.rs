use serde::{Deserialize, Serialize};
use yew_router::Routable;

pub mod activity_feed;
pub mod alerts;
pub mod code;
pub mod components;
pub mod songs;

#[derive(Clone, Routable, PartialEq)]
pub enum Route {
    #[at("/alerts")]
    Alerts,
    #[at("/activity")]
    Activity,
    #[at("/")]
    Songs,
    #[at("/code")]
    Code,
    #[not_found]
    #[at("/404")]
    NotFound,
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

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub enum AlertEventType {
    #[default]
    Nothing,
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
        subscribed_for: u64,
        streak: u64,
    },
    GiftSub {
        gifter: String,
        total: u64,
        tier: String,
    },
    GiftedSub {
        gifted: String,
        tier: String,
    },
    Bits {
        message: String,
        is_anonymous: bool,
        cheerer: String,
        bits: u64,
    },
}

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct Alert {
    new: bool,
    r#type: AlertEventType,
}
