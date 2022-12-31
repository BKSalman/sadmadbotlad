use futures_util::{pin_mut, StreamExt};
use obws::{events::Event, Client};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    event_handler::{Event as EventHandler, IrcChat, IrcEvent},
    twitch::{run_ads, AdError},
    APP,
};

pub async fn obs_websocket(e_sender: UnboundedSender<EventHandler>) -> eyre::Result<()> {
    let api_info = APP.get().await.api_info.clone();

    let Ok(client) = Client::connect("localhost", 4455, Some(&api_info.obs_server_password)).await else {
        println!("Could not connect to obs websocket");
        return Ok(())
    };

    let events = client.events()?;
    pin_mut!(events);

    while let Some(event) = events.next().await {
        if let Event::CurrentProgramSceneChanged { name } = event {
            if name != "Random" {
                continue;
            }

            match run_ads(&api_info).await {
                Ok(retry) => {
                    println!("retry after {} seconds", retry);
                    e_sender.send(EventHandler::IrcEvent(IrcEvent::Chat(IrcChat::Commercial)))?;
                }
                Err(e) => match e {
                    AdError::TooManyRequests => {
                        println!("{}", e)
                    }
                    AdError::UnAuthorized => panic!("{e}"),
                    AdError::RequestErr(err) => panic!("{}", err),
                },
            }
        }
    }

    Ok(())
}
