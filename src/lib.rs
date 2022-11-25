use eyre::WrapErr;
use serde::Deserialize;
use tokio::task::JoinHandle;

pub mod discord;
pub mod twitch;

#[derive(Deserialize)]
pub struct ApiInfo {
    pub google_api_key: String,
    pub user: String,
    pub client_id: String,
    pub twitch_oauth: String,
    pub discord_token: String,
}

impl ApiInfo {
    pub fn new() -> Self {
        match envy::from_env::<ApiInfo>() {
            Ok(api_info) => api_info,
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

