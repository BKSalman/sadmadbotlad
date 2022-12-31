use std::io::BufRead;
use std::sync::Arc;

use eyre::Context;
use sadmadbotlad::obs_websocket::obs_websocket;
use sadmadbotlad::twitch::get_access_token_from_code;
use sadmadbotlad::{event_handler, sr_ws_server::sr_ws_server, ApiInfo};
use sadmadbotlad::{set_cmd_delim, Alert, SrFrontEndEvent};
// use tokio_retry::{strategy::ExponentialBackoff, Retry};

use sadmadbotlad::{flatten, ws_server::ws_server};

use sadmadbotlad::{eventsub::eventsub, install_eyre, irc::irc_connect};

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    install_eyre()?;

    let args = std::env::args();

    for arg in args {
        match arg.as_str() {
            "-m" | "--manual" => {
                let auth_link = std::fs::read_to_string("auth_link.txt")?;
                open::that(auth_link)?;

                let mut code = String::new();
                let stdin = std::io::stdin();
                println!("Enter code:");
                stdin.lock().read_line(&mut code).unwrap();

                let code = code.trim();

                get_access_token_from_code(code).await?;
            }
            "-t" | "--test" => {
                // safety: COMMAND_DELIMETER is only changed before running the function run,
                // which only has readers to the variable
                set_cmd_delim("#");
            }
            _ => {}
        }
    }

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
        flatten(tokio::spawn(irc_connect(
            sender.clone(),
            sr_sender.clone(),
            e_sender.clone(),
            e_receiver,
            api_info.clone()
        ))),
        flatten(tokio::spawn(sr_ws_server(sr_sender))),
        flatten(tokio::spawn(ws_server(sender))),
        flatten(tokio::spawn(obs_websocket(e_sender, api_info))),
        // TODO: get current spotify song every 20 secs
    )
    .wrap_err_with(|| "Run")?;

    Ok(())
}
