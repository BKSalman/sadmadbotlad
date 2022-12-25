use std::sync::Arc;

use eyre::Context;
use sadmadbotlad::{SrFrontEndEvent, Alert};
// TODO: queue in front end
use sadmadbotlad::{event_handler, ApiInfo, sr_ws_server::sr_ws_server};
use sadmadbotlad::obs_websocket::obs_websocket;
// use tokio_retry::{strategy::ExponentialBackoff, Retry};

use sadmadbotlad::{flatten, ws_server::ws_server};

use sadmadbotlad::{eventsub::eventsub, install_eyre, irc::irc_connect};

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    install_eyre()?;

    // Retry::spawn(ExponentialBackoff::from_millis(100).take(2), || {
    //     println!("Attempting...");
    //     run()
    // })
    //     .await
    //     .with_context(|| "main:: running application")?;

    run().await?;

    Ok(())
}

async fn run() -> Result<(), eyre::Report> {
    let (sender, _) = tokio::sync::broadcast::channel::<Alert>(100);
    let (sr_sender, _) = tokio::sync::broadcast::channel::<SrFrontEndEvent>(100);
    let (e_sender, e_receiver) = tokio::sync::mpsc::unbounded_channel::<event_handler::Event>();

    let api_info = Arc::new(ApiInfo::new().await?);

    tokio::try_join!(
        flatten(tokio::spawn(eventsub(sender.clone(), api_info.clone()))),
        flatten(tokio::spawn(irc_connect(sender.clone(), sr_sender.clone(), e_sender.clone(), e_receiver, api_info.clone()))),
        flatten(tokio::spawn(sr_ws_server(sr_sender))),
        flatten(tokio::spawn(ws_server(sender))),
        flatten(tokio::spawn(obs_websocket(e_sender, api_info))),
        // TODO: get current spotify song every 20 secs
    )
    .wrap_err_with(|| "Run")?;

    Ok(())
}
