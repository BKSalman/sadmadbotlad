use eyre::WrapErr;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use twitch_api::types::{self, Timestamp};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveStatus {
    Live {
        started_at: types::Timestamp,
        url: String,
    },
    Offline {
        url: String,
    },
}

impl LiveStatus {
    /// Returns `true` if the live status is [`Live`].
    ///
    /// [`Live`]: LiveStatus::Live
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Live { .. })
    }

    /// Returns `true` if the live status is [`Offline`].
    ///
    /// [`Offline`]: LiveStatus::Offline
    pub fn is_offline(&self) -> bool {
        matches!(self, Self::Offline { .. })
    }

    pub fn to_message(&self) -> eyre::Result<tokio_tungstenite::tungstenite::Message> {
        #[derive(serde::Serialize)]
        struct Msg {
            html: String,
            live: bool,
        }
        let msg = match self {
            Self::Live { .. } => Msg {
                html: "Yes".to_string(),
                live: true,
            },
            Self::Offline { .. } => Msg {
                html: "No".to_string(),
                live: false,
            },
        };
        Ok(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::to_string(&msg).wrap_err_with(|| "could not make into a message")?,
        ))
    }
}

#[derive(Debug, Deserialize)]
pub struct WsEventSub {
    pub metadata: WsMetaData,
    pub payload: WsPayload,
}

#[allow(unused)]
#[derive(Debug, Deserialize)]
pub struct WsMetaData {
    message_id: String,
    message_type: String,
    message_timestamp: Timestamp,
    subscription_type: Option<String>,
    subscription_version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WsPayload {
    pub session: Option<WsSession>,
    pub subscription: Option<WsSubscription>,
    pub event: Option<WsEvent>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WsSubscription {
    pub id: String,
    pub status: String,
    pub r#type: String,
    version: String,
    condition: WsCondition,
    transport: WsTransport,
    created_at: Timestamp,
    cost: u8,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WsCondition {
    broadcaster_user_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WsTransport {
    method: String,
    session_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WsSession {
    pub id: String,
    pub status: String,
    pub connected_at: Timestamp,
    pub keepalive_timeout_seconds: u32,
    pub reconnect_url: Option<String>,
}

#[allow(unused)]
#[derive(Debug, Deserialize)]
pub struct WsEvent {
    user_id: Option<String>,
    user_login: Option<String>,
    user_name: Option<String>,
    broadcaster_user_id: String,
    broadcaster_user_login: String,
    broadcaster_user_name: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EventSub {
    r#type: String,
    version: u8,
    condition: EventSubCondition,
    transport: WsTransport,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EventSubCondition {
    broadcaster_user_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EventSubResponse {
    pub data: Vec<WsSubscription>,
    total_cost: u16,
    max_total_cost: u32,
    pagination: Pagination,
}

#[derive(Debug, Deserialize, Serialize)]
struct Pagination {
    cursor: Option<String>,
}

#[derive(Deserialize)]
pub struct TwitchApiResponse {
    pub data: Vec<TwitchChannelInfo>
}

#[derive(Deserialize)]
pub struct TwitchChannelInfo {
    pub broadcaster_id: String,
    pub broadcaster_login: String,
    pub broadcaster_name: String,
    pub broadcaster_language: String,
    pub game_id: String,
    pub game_name: String,
    pub title: String,
    pub delay: u32,
}

pub fn online_event(session_id: String) -> Value {
    json!({
        "type": "stream.online",
        "version": "1",
        "condition": {
            "broadcaster_user_id": "143306668" //110644052
        },
        "transport": {
            "method": "websocket",
            "session_id": session_id
        }
    })
}

pub fn offline_event(session_id: String) -> Value {
    json!({
        "type": "stream.offline",
        "version": "1",
        "condition": {
            "broadcaster_user_id": "143306668"
        },
        "transport": {
            "method": "websocket",
            "session_id": session_id
        }
    })
}
