use std::sync::Arc;

use eyre::Context;
use sadmadbotlad::db::Store;
use sadmadbotlad::obs_websocket::obs_websocket;
use sadmadbotlad::twitch::access_token;
use sadmadbotlad::{event_handler, sr_ws_server::sr_ws_server};
use sadmadbotlad::{AlertEventType, ApiInfo, APP};

use sadmadbotlad::{flatten, ws_server::ws_server};

use sadmadbotlad::{eventsub::eventsub, install_eyre, irc::irc_connect};

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    install_eyre()?;

    if APP.config.manual {
        access_token().await?;
    }

    let store = Store::new().await?;

    let alert = AlertEventType::Follow {
        follower: String::from("someone"),
    };

    // store.delete_events_table().await?;

    store.new_event(alert).await?;

    let res = store.get_events().await?;

    println!("{res:#?}");

    // run().await?;
    Ok(())
}

async fn run() -> Result<(), eyre::Report> {
    let api_info = Arc::new(ApiInfo::new().await.expect("Api info failed"));

    let (e_sender, e_receiver) = tokio::sync::mpsc::unbounded_channel::<event_handler::Event>();

    tokio::try_join!(
        flatten(tokio::spawn(eventsub(api_info.clone()))),
        flatten(tokio::spawn(irc_connect(
            e_sender.clone(),
            e_receiver,
            api_info.clone(),
        ))),
        flatten(tokio::spawn(sr_ws_server())),
        flatten(tokio::spawn(ws_server())),
        flatten(tokio::spawn(obs_websocket(e_sender, api_info))),
        // TODO: get current spotify song every 20 secs
    )
    .wrap_err_with(|| "Run")?;

    Ok(())
}
