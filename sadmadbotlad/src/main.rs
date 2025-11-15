use axum::Router;
use axum::http::StatusCode;
use axum::routing::get_service;
use futures_util::TryFutureExt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use sadmadbotlad::db::Store;
use sadmadbotlad::eventsub::eventsub;
use sadmadbotlad::irc::irc_connect;
use sadmadbotlad::obs_websocket::obs_websocket;
use sadmadbotlad::song_requests::{QueueMessages, SongRequest, SrQueue};
use sadmadbotlad::sr_ws_server::sr_ws_server;
use sadmadbotlad::twitch::{TwitchToken, TwitchTokenMessages, access_token};
use sadmadbotlad::ws_server::ws_server;
use sadmadbotlad::{APP, Alert, ApiInfo, MainError, flatten, logging};

#[tokio::main]
async fn main() -> Result<(), MainError> {
    logging();

    let mut api_info = ApiInfo::new().expect("Api info failed");

    if APP.config.manual {
        access_token(&mut api_info.twitch).await?;

        std::fs::write(&APP.config.config_path, toml::to_string(&api_info).unwrap())?;
    }

    if let Err(e) = tokio::try_join!(
        run(api_info),
        run_frontend(APP.config.frontend_port, &APP.config.static_path)
    ) {
        eprintln!("Sadmadladbot failed: {e}");
    }

    Ok(())
}

async fn run(api_info: ApiInfo) -> Result<(), MainError> {
    let api_info = Arc::new(api_info);

    let (token_sender, token_receiver) = mpsc::unbounded_channel::<TwitchTokenMessages>();

    let twitch = TwitchToken::new(api_info.twitch.clone(), token_receiver);

    let (queue_sender, queue_receiver) = mpsc::unbounded_channel::<QueueMessages>();

    let (song_sender, song_receiver) = tokio::sync::mpsc::channel::<SongRequest>(200);

    let (alerts_sender, _) = tokio::sync::broadcast::channel::<Alert>(100);

    let queue = SrQueue::new(api_info.clone(), song_sender, queue_receiver);

    let store = Arc::new(Store::new().await?);

    tokio::try_join!(
        flatten(tokio::spawn(
            eventsub(
                alerts_sender.clone(),
                token_sender.clone(),
                api_info.clone(),
                store.clone()
            )
            .map_err(Into::into)
        )),
        flatten(tokio::spawn(twitch.handle_messages().map_err(Into::into))),
        flatten(tokio::spawn(queue.handle_messages().map_err(Into::into))),
        flatten(tokio::spawn(
            sr_ws_server(queue_sender.clone()).map_err(Into::into)
        )),
        flatten(tokio::spawn(
            irc_connect(
                alerts_sender.clone(),
                song_receiver,
                queue_sender.clone(),
                token_sender.clone(),
                store.clone(),
            )
            .map_err(Into::into)
        )),
        flatten(tokio::spawn(
            ws_server(alerts_sender, store).map_err(Into::into)
        )),
        flatten(tokio::spawn(
            obs_websocket(
                // e_sender,
                token_sender,
                api_info
            )
            .map_err(Into::into)
        ))
    )?;

    Ok(())
}

async fn run_frontend(port: u16, static_path: impl AsRef<Path>) -> Result<(), MainError> {
    let static_path = static_path.as_ref();

    let router = Router::new().fallback_service(
        Router::new().nest_service(
            "/",
            get_service(ServeDir::new(static_path).fallback(ServeFile::new(
                PathBuf::from(static_path).join("index.html"),
            )))
            .handle_error(|error| async move {
                tracing::error!(?error, "failed serving static file");
                StatusCode::INTERNAL_SERVER_ERROR
            })
            .layer(TraceLayer::new_for_http()),
        ),
    );

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port);

    let listener = TcpListener::bind(addr).await?;

    axum::serve(listener, router).await?;

    Ok(())
}
