use std::{fs, io::Read};

use eyre::WrapErr;
use serde::Deserialize;
use song_requests::Queue;
use tokio::task::JoinHandle;

pub mod discord;
pub mod event_handler;
pub mod eventsub;
pub mod irc;
pub mod song_requests;
pub mod twitch;
pub mod ws_server;
pub mod youtube;
pub mod commands;

pub fn install_eyre() -> eyre::Result<()> {
    let (_, eyre_hook) = color_eyre::config::HookBuilder::default().into_hooks();

    eyre_hook.install()?;

    std::panic::set_hook(Box::new(move |pi| {
        eprintln!("{pi}");
    }));
    Ok(())
}

#[derive(Debug, Clone)]
pub enum FrontEndEvent {
    Follow {
        follower: String,
    },
    Raid {
        from: String,
    },
    QueueRequest,
    SongsResponse(Queue),
}

#[derive(Deserialize)]
pub struct ApiInfo {
    pub google_api_key: String,
    pub user: String,
    pub client_id: String,
    pub client_secret: String,
    pub twitch_access_token: String,
    pub twitch_refresh_token: String,
    pub discord_token: String,
}

impl ApiInfo {
    pub fn new() -> Result<Self, eyre::Report> {
        let Ok(mut config) = fs::File::open("config.toml") else {
            panic!("no config file");
        };

        let mut config_str = String::new();
        config.read_to_string(&mut config_str).expect("config str");

        match toml::from_str::<ApiInfo>(&config_str) {
            Ok(api_info) => {
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
