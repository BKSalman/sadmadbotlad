use std::io::BufRead;
use std::sync::Arc;

use eyre::Context;
use sadmadbotlad::obs_websocket::obs_websocket;
use sadmadbotlad::twitch::get_access_token_from_code;
use sadmadbotlad::{event_handler, sr_ws_server::sr_ws_server, ApiInfo};
use sadmadbotlad::{Alert, App, SrFrontEndEvent, APP};
// use tokio_retry::{strategy::ExponentialBackoff, Retry};

use sadmadbotlad::{flatten, ws_server::ws_server};

use sadmadbotlad::{eventsub::eventsub, install_eyre, irc::irc_connect};

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    install_eyre()?;

    if APP.get().await.config.manual {
        let auth_link = std::fs::read_to_string("auth_link.txt")?;
        open::that(auth_link)?;

        let mut code = String::new();
        let stdin = std::io::stdin();
        println!("Enter code:");
        stdin.lock().read_line(&mut code).unwrap();

        let code = code.trim();

        get_access_token_from_code(code).await?;
    }

    println!("{:?}", APP.get().await.config);

    // run().await?;
    Ok(())
}

async fn run() -> Result<(), eyre::Report> {
    let (e_sender, e_receiver) = tokio::sync::mpsc::unbounded_channel::<event_handler::Event>();

    tokio::try_join!(
        flatten(tokio::spawn(eventsub())),
        flatten(tokio::spawn(irc_connect(e_sender.clone(), e_receiver))),
        flatten(tokio::spawn(sr_ws_server())),
        flatten(tokio::spawn(ws_server())),
        flatten(tokio::spawn(obs_websocket(e_sender))),
        // TODO: get current spotify song every 20 secs
    )
    .wrap_err_with(|| "Run")?;

    Ok(())
}
