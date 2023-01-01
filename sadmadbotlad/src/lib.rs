use std::{fs, io::Read, sync::Arc};

use clap::{command, Parser};
use eyre::WrapErr;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use song_requests::SrQueue;
use tokio::task::JoinHandle;
use twitch::refresh_access_token;

pub mod commands;
pub mod discord;
pub mod event_handler;
pub mod eventsub;
pub mod irc;
pub mod obs_websocket;
pub mod song_requests;
pub mod sr_ws_server;
pub mod twitch;
pub mod ws_server;
pub mod youtube;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    #[arg(short, long, default_value_t = false)]
    pub manual: bool,
    #[arg(short, long, default_value_t = '!')]
    pub cmd_delim: char,
    #[arg(short, long, default_value_t = 3000)]
    pub port: u16,
}

#[derive(Debug)]
pub struct App {
    pub config: Config,
    pub alerts_sender: tokio::sync::broadcast::Sender<Alert>,
    pub sr_sender: tokio::sync::broadcast::Sender<SrFrontEndEvent>,
}

impl App {
    pub fn new() -> Self {
        let (alerts_sender, _) = tokio::sync::broadcast::channel::<Alert>(100);
        let (sr_sender, _) = tokio::sync::broadcast::channel::<SrFrontEndEvent>(100);

        Self {
            config: Config::parse(),
            alerts_sender,
            sr_sender,
        }
    }
}

lazy_static! {
    pub static ref APP: App = App::new();
}

pub fn install_eyre() -> eyre::Result<()> {
    let (_, eyre_hook) = color_eyre::config::HookBuilder::default().into_hooks();

    eyre_hook.install()?;

    std::panic::set_hook(Box::new(move |pi| {
        eprintln!("{pi}");
    }));
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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
        subscribed_for: u64,
        streak: u64,
    },
    GiftSub {
        gifter: String,
        total: u64,
        tier: String,
    },
    Gifted {
        gifted: String,
        tier: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Alert {
    new: bool,
    alert_type: AlertEventType,
}

#[derive(Debug, Clone)]
pub enum SrFrontEndEvent {
    QueueRequest,
    QueueResponse(Arc<tokio::sync::RwLock<SrQueue>>),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiInfo {
    pub google_api_key: String,
    pub user: String,
    pub client_id: String,
    pub client_secret: String,
    pub twitch_access_token: String,
    pub twitch_refresh_token: String,
    pub discord_token: String,
    pub obs_server_password: String,
}

impl ApiInfo {
    pub async fn new() -> eyre::Result<Self> {
        let Ok(mut config) = fs::File::open("config.toml") else {
            panic!("no config file");
        };

        let mut config_str = String::new();
        config.read_to_string(&mut config_str).expect("config str");

        match toml::from_str::<ApiInfo>(&config_str) {
            Ok(mut api_info) => {
                refresh_access_token(&mut api_info).await?;
                Ok(api_info)
            }
            Err(e) => {
                panic!("Api Info:: {e}");
            }
        }
    }
}

pub async fn flatten<T>(handle: JoinHandle<Result<T, eyre::Report>>) -> Result<T, eyre::Report> {
    match handle.await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(err),
        Err(e) => Err(e).wrap_err_with(|| "handling failed"),
    }
}

// lazy man's macros

#[macro_export]
macro_rules! map {
    // map-like
    ($($k:expr => $v:expr),* $(,)?) => {{
        HashMap::from([$(($k, $v),)*])
    }};
    // set-like
    ($($v:expr),* $(,)?) => {{
        HashMap::from([$($v,)*])
    }};
}

#[macro_export]
macro_rules! string {
    ($s: expr) => {{
        String::from($s)
    }};
}
