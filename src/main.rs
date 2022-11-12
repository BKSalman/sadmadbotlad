#![allow(unused)]
use std::sync::Arc;

use axum::{routing::post, Extension, Router};
use envy;
use eyre::WrapErr;
use serde::Deserialize;
use tokio::task::JoinHandle;
use twitch_api::{client::ClientDefault, HelixClient};

mod discord;
mod twitch;
mod util;

// use discord::send_notification;
use util::install_eyre;

#[derive(Debug, Clone, Deserialize)]
pub struct ApiInfo {
    /// Client ID of twitch application
    pub client_id: twitch_oauth2::ClientId,
    /// Client Secret of twitch application
    pub client_secret: twitch_oauth2::ClientSecret,
    pub broadcaster_login: twitch_api::types::UserName,
    pub website_callback: String,
    pub website: String,
    secret: String,
}

impl ApiInfo {
    pub fn secret(&self) -> &[u8] {
        return self.secret.as_bytes();
    }

    pub fn secret_str(&self) -> &str {
        return &self.secret;
    }
}

#[derive(Debug)]
pub struct Config {
    broadcaster: twitch_api::helix::users::User,
    broadcaster_url: String,
    website_url: String,
}

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    // send_notification().await?;
    install_eyre()?;

    match envy::from_env::<ApiInfo>() {
        Ok(api_info) => {
            run(api_info)
                .await
                .with_context(|| "when running application")?;
        }
        Err(e) => panic!("{e}"),
    }

    Ok(())
}

async fn run(api_info: ApiInfo) -> Result<(), eyre::Report> {
    let client: HelixClient<'static, _> = twitch_api::HelixClient::with_client(
        <reqwest::Client>::default_client_with_name(Some(
            "twitch-rs/eventsub"
                .parse()
                .wrap_err_with(|| "when creating header name")
                .unwrap(),
        ))
        .wrap_err_with(|| "when creating client")?,
    );

    let token = twitch_oauth2::AppAccessToken::get_app_access_token(
        &client,
        api_info.client_id.clone(),
        api_info.client_secret.clone(),
        vec![],
    )
    .await?;

    let broadcaster = client
        .get_user_from_login(&api_info.broadcaster_login, &token)
        .await?
        .ok_or_else(|| eyre::eyre!("broadcaster not found"))?;

    let config = Arc::new(Config {
        broadcaster_url: format!("https://twitch.tv/{}", broadcaster.login),
        broadcaster,
        website_url: api_info.website.clone(),
    });

    let token = Arc::new(tokio::sync::RwLock::new(token));

    let app = Router::new()
        .route(
            "/twitch/eventsub",
            post(move |config, api_info, request| twitch::eventsub(config, api_info, request)),
        )
        .layer(
            tower::ServiceBuilder::new()
                .layer(Extension(client.clone()))
                .layer(Extension(config.clone()))
                .layer(Extension(Arc::new(api_info.clone()))),
        );

    let server = tokio::spawn(async move {
        axum::Server::bind(
            &"0.0.0.0:4000"
                .parse()
                .wrap_err_with(|| "when parsing address")?,
        )
        .serve(app.into_make_service())
        .await
        .wrap_err_with(|| "when serving")?;
        Ok::<(), eyre::Report>(())
    });

    let run = tokio::try_join!(
        flatten(server),
        flatten(tokio::spawn(twitch::eventsub_register(
            token.clone(),
            config.clone(),
            client.clone(),
            api_info.website_callback.clone(),
            api_info.secret_str().to_string(),
        ))),
    );

    run?;

    Ok(())
}

async fn flatten<T>(handle: JoinHandle<Result<T, eyre::Report>>) -> Result<T, eyre::Report> {
    match handle.await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(err),
        Err(e) => Err(e).wrap_err_with(|| "handling failed"),
    }
}
