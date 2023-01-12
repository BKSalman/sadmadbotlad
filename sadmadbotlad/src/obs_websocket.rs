use std::{sync::Arc, time::Duration};

use futures_util::{pin_mut, StreamExt};
use obws::{events::Event, Client};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    event_handler::{Event as EventHandler, IrcChat, IrcEvent},
    twitch::{run_ads, AdError},
    ApiInfo,
};

pub async fn obs_websocket(
    e_sender: UnboundedSender<EventHandler>,
    api_info: Arc<ApiInfo>,
) -> eyre::Result<()> {
    let Ok(client) = Client::connect("localhost", 4455, Some(&api_info.obs_server_password)).await else {
        println!("Could not connect to obs websocket");
        return Ok(())
    };

    if let Err(e) = refresh_alert_box(&client).await {
        println!("{e}");
    }

    let events = client.events()?;
    pin_mut!(events);

    while let Some(event) = events.next().await {
        if let Event::CurrentProgramSceneChanged { name } = event {
            if let Err(e) = refresh_alert_box(&client).await {
                println!("{e}");
            }

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

async fn refresh_alert_box(client: &Client) -> eyre::Result<()> {
    let current_scene = client.scenes().current_program_scene().await?;

    let id = client
        .scene_items()
        .id(obws::requests::scene_items::Id {
            scene: &current_scene,
            source: "AlertBox",
            search_offset: None,
        })
        .await?;

    client
        .scene_items()
        .set_enabled(obws::requests::scene_items::SetEnabled {
            scene: &current_scene,
            item_id: id,
            enabled: false,
        })
        .await?;

    tokio::time::sleep(Duration::from_millis(500)).await;

    client
        .scene_items()
        .set_enabled(obws::requests::scene_items::SetEnabled {
            scene: &current_scene,
            item_id: id,
            enabled: true,
        })
        .await?;

    Ok(())
}
