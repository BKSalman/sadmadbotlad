use std::sync::Arc;

use eyre::Context;
use sadmadbotlad::db::Store;
use sadmadbotlad::obs_websocket::obs_websocket;
use sadmadbotlad::twitch::{access_token, TwitchToken, TwitchTokenMessages};
use sadmadbotlad::ws_server::ws_server;
use sadmadbotlad::{event_handler, sr_ws_server::sr_ws_server};
use sadmadbotlad::{eventsub::eventsub, install_eyre, irc::irc_connect};
use sadmadbotlad::{flatten, ApiInfo, APP};
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

    let (e_sender, e_receiver) = mpsc::unbounded_channel::<event_handler::Event>();

    let (token_sender, token_receiver) = mpsc::unbounded_channel::<TwitchTokenMessages>();

    let twitch = TwitchToken::new(api_info.twitch.clone(), token_receiver);

    let store = Arc::new(Store::new().await?);

    tokio::try_join!(
        flatten(tokio::spawn(eventsub(
            token_sender,
            api_info.clone(),
            store.clone()
        ))),
        flatten(tokio::spawn(twitch.handle_messages())),
        flatten(tokio::spawn(sr_ws_server())),
        flatten(tokio::spawn(irc_connect(
            e_sender.clone(),
            e_receiver,
            api_info.clone(),
            store.clone(),
        ))),
        flatten(tokio::spawn(ws_server(store))),
        flatten(tokio::spawn(obs_websocket(e_sender, api_info)))
    )
    .wrap_err_with(|| "Run")?;

    Ok(())
}

/*
    enum Event {
        // something
        SendSoemthingAndGetResponse(Something, oneshot),
    }

    let (irc_sender, irc_receiver) = mpsc::channel::<Event>();
    let (token_sender, token_receiver) = mpsc::channel::<Event>();

    actor1(irc_sender);
    tokio::spawn(actor2(irc_sender));

    fn actor1(irc_sender) {
        let (sender, receiver) = oneshot::channel();
        irc_sender.send(Event(something, sender))
        let Some(res) = receiver.next().await?;
    }

    fn actor2(irc_receiver) {
        while let Some((request, one_shot)) = irc_receiver.next().await {
            let res = request.execute();
            one_shot.send(res);
        }
    }
*/
