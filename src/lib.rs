use std::fs;

use eyre::WrapErr;
use serde::Deserialize;
use tokio::task::JoinHandle;

pub mod discord;
pub mod event_handler;
pub mod eventsub;
pub mod irc;
pub mod song_requests;
pub mod twitch;
pub mod ws_server;
pub mod youtube;

#[derive(Debug, Clone)]
pub enum FrontEndEvent {
    Follow {
        follower: String,
    },
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
        match envy::from_env::<ApiInfo>() {
            Ok(mut api_info) => {
                let reader = fs::read_to_string("refresh_token.txt")?;
                let tokens = reader.split('\n').collect::<Vec<&str>>();

                let refresh_token = tokens[0].to_string();
                let access_token = tokens[1].to_string();

                api_info.twitch_refresh_token = refresh_token;
                api_info.twitch_access_token = access_token;
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
