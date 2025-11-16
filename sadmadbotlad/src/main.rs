use anyhow::Context;
use axum::Router;
use axum::http::StatusCode;
use axum::routing::get_service;
use sadmadbotlad::db::{DBMessage, Store};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use sadmadbotlad::eventsub::eventsub;
use sadmadbotlad::irc::irc_connect;
use sadmadbotlad::obs_websocket::obs_websocket;
use sadmadbotlad::song_requests::{QueueMessages, SongRequest, SrQueue, play_song, setup_mpv};
use sadmadbotlad::sr_ws_server::sr_ws_server;
use sadmadbotlad::twitch::{TwitchToken, TwitchTokenMessages, access_token};
use sadmadbotlad::ws_server::ws_server;
use sadmadbotlad::{APP, Alert, ApiInfo, flatten, logging};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging();

    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .unwrap();

    let mut api_info = ApiInfo::new().expect("Api info failed");

    let front_end = tokio::spawn(async move {
        run_frontend(APP.config.frontend_port, &APP.config.static_path).await
    });

    if APP.config.manual {
        access_token(&mut api_info.twitch).await?;

        std::fs::write(&APP.config.config_path, toml::to_string(&api_info).unwrap())?;
    }

    if let Err(e) = tokio::try_join!(flatten(front_end), run(api_info),) {
        tracing::error!("Sadmadladbot failed: {e:?}");
    }

    Ok(())
}

async fn run(api_info: ApiInfo) -> anyhow::Result<()> {
    let api_info = Arc::new(api_info);

    let (token_request_sender, token_request_receiver) =
        mpsc::unbounded_channel::<TwitchTokenMessages>();

    let twitch = TwitchToken::new(api_info.twitch.clone(), token_request_receiver);

    let (queue_sender, queue_receiver) = mpsc::unbounded_channel::<QueueMessages>();

    let (song_sender, song_receiver) = tokio::sync::mpsc::channel::<SongRequest>(200);

    let (alerts_sender, _) = tokio::sync::broadcast::channel::<Alert>(100);

    let (db_tx, db_rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let store = Store::new().unwrap();
        while let Ok(message) = db_rx.recv() {
            match message {
                DBMessage::NewEvent(alert_event_type) => {
                    store.new_event(alert_event_type).unwrap();
                }
                DBMessage::GetEvent(id, one_shot_sender) => {
                    one_shot_sender
                        .send(store.get_event(id).unwrap().unwrap())
                        .unwrap();
                }
                DBMessage::GetEvents(one_shot_sender) => {
                    one_shot_sender.send(store.get_events().unwrap()).unwrap();
                }
            }
        }
    });

    let mpv = Arc::new(setup_mpv());

    let queue = SrQueue::new(api_info.clone(), song_sender, queue_receiver);

    {
        let queue_sender = queue_sender.clone();
        let mpv = mpv.clone();
        std::thread::spawn(move || play_song(mpv, song_receiver, queue_sender));
    }

    tokio::try_join!(
        flatten(tokio::spawn({
            let alerts_sender = alerts_sender.clone();
            let token_sender = token_request_sender.clone();
            let db_tx = db_tx.clone();
            let api_info = api_info.clone();
            async move {
                eventsub(
                    alerts_sender.clone(),
                    token_sender.clone(),
                    api_info.clone(),
                    db_tx.clone(),
                )
                .await
                .with_context(|| "eventsub")
            }
        })),
        flatten(tokio::spawn(async move {
            twitch.handle_messages().await.with_context(|| "twitch")
        })),
        flatten(tokio::spawn(async move {
            queue.handle_messages().await.with_context(|| "queue")
        })),
        flatten(tokio::spawn({
            let queue_sender = queue_sender.clone();
            async move {
                sr_ws_server(queue_sender.clone())
                    .await
                    .with_context(|| "sr_ws_server")
            }
        })),
        flatten(tokio::spawn({
            let alerts_sender = alerts_sender.clone();
            let token_sender = token_request_sender.clone();
            let db_tx = db_tx.clone();
            async move {
                irc_connect(
                    alerts_sender.clone(),
                    queue_sender,
                    token_sender.clone(),
                    db_tx.clone(),
                    mpv,
                )
                .await
                .with_context(|| "irc_connect")
            }
        })),
        flatten(tokio::spawn(async move {
            ws_server(alerts_sender, db_tx)
                .await
                .with_context(|| "ws_server")
        })),
        flatten(tokio::spawn(async move {
            obs_websocket(
                // e_sender,
                token_request_sender,
                api_info,
            )
            .await
            .with_context(|| "obs_websocket")
        }))
    )?;

    Ok(())
}

async fn run_frontend(port: u16, static_path: impl AsRef<Path>) -> anyhow::Result<()> {
    let static_path = static_path.as_ref();

    let router = Router::new().fallback_service(
        Router::new().fallback_service(
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

    tracing::info!("running frontend on port {port}");

    let listener = TcpListener::bind(addr).await?;

    axum::serve(listener, router).await?;

    Ok(())
}
