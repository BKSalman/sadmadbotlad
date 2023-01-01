use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;

use eyre::Context;
use futures_util::StreamExt;
use sadmadbotlad::obs_websocket::obs_websocket;
use sadmadbotlad::twitch::get_access_token_from_code;
use sadmadbotlad::{event_handler, sr_ws_server::sr_ws_server};
use sadmadbotlad::{ApiInfo, APP};

use sadmadbotlad::{flatten, ws_server::ws_server};

use sadmadbotlad::{eventsub::eventsub, install_eyre, irc::irc_connect};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite};

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

async fn access_token() -> eyre::Result<()> {
    let auth_link = std::fs::read_to_string("auth_link.txt")?;
    open::that(auth_link)?;

    let ip_address = Ipv4Addr::new(127, 0, 0, 1);
    let address = SocketAddrV4::new(ip_address, 4040);
    let listener = TcpListener::bind(address).await?;

    let Ok((stream, _)) = listener.accept().await else {
        return Err(eyre::eyre!("Code Websocket failed"))
    };

    let peer = stream
        .peer_addr()
        .expect("connected streams should have a peer address");

    println!("Peer address: {}", peer);

    let mut ws = accept_async(stream).await?;

    if let Some(msg) = ws.next().await {
        match msg {
            Ok(tungstenite::Message::Text(code)) => {
                get_access_token_from_code(&code).await?;
            }
            _ => println!("something wierd happened"),
        }
    }

    Ok(())
}
