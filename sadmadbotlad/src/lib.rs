use std::{fs, io::Read};

use eyre::WrapErr;
use serde::{Deserialize, Serialize};
use song_requests::Queue;
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
    Follow { follower: String },
    Raid { from: String },
    Subscribe { subscriber: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Alert {
    new: bool,
    alert_type: AlertEventType,
}

#[derive(Debug, Clone)]
pub enum SrFrontEndEvent {
    QueueRequest,
    QueueResponse(Queue),
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
