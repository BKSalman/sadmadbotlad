use std::sync::Arc;

use eyre::Context;
use sadmadbotlad::db::Store;
use sadmadbotlad::obs_websocket::obs_websocket;
use sadmadbotlad::song_requests::{QueueMessages, SongRequest, SrQueue};
use sadmadbotlad::sr_ws_server::sr_ws_server;
use sadmadbotlad::twitch::{access_token, TwitchToken, TwitchTokenMessages};
use sadmadbotlad::ws_server::ws_server;
use sadmadbotlad::{eventsub::eventsub, install_eyre, irc::irc_connect};
use sadmadbotlad::{flatten, Alert, ApiInfo, APP};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    install_eyre()?;

    if APP.config.manual {
        access_token().await?;
    }

    run().await?;
    Ok(())
}

async fn run() -> Result<(), eyre::Report> {
    let api_info = Arc::new(ApiInfo::new().await.expect("Api info failed"));

    let (token_sender, token_receiver) = mpsc::unbounded_channel::<TwitchTokenMessages>();

    let twitch = TwitchToken::new(api_info.twitch.clone(), token_receiver);

    let (queue_sender, queue_receiver) = mpsc::unbounded_channel::<QueueMessages>();

    let (song_sender, song_receiver) = tokio::sync::mpsc::channel::<SongRequest>(200);

    let (alerts_sender, _) = tokio::sync::broadcast::channel::<Alert>(100);

    let queue = SrQueue::new(api_info.clone(), song_sender, queue_receiver);

    let store = Arc::new(Store::new().await?);

    tokio::try_join!(
        flatten(tokio::spawn(eventsub(
            alerts_sender.clone(),
            token_sender.clone(),
            api_info.clone(),
            store.clone()
        ))),
        flatten(tokio::spawn(twitch.handle_messages())),
        flatten(tokio::spawn(queue.handle_messages())),
        flatten(tokio::spawn(sr_ws_server(queue_sender.clone()))),
        flatten(tokio::spawn(irc_connect(
            alerts_sender.clone(),
            song_receiver,
            queue_sender.clone(),
            token_sender.clone(),
            store.clone(),
        ))),
        flatten(tokio::spawn(ws_server(alerts_sender, store))),
        flatten(tokio::spawn(obs_websocket(
            // e_sender,
            token_sender,
            api_info
        )))
    )
    .wrap_err_with(|| "Run")?;

    Ok(())
}
