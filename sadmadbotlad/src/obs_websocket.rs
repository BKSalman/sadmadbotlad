use std::{sync::Arc, time::Duration};

use futures_util::{pin_mut, StreamExt};
use obws::{events::Event, Client};
use tokio::sync::mpsc;

use crate::{
    // event_handler::{Event as EventHandler, IrcChat, IrcEvent},
    twitch::{run_ads, TwitchTokenMessages},
    ApiInfo,
};

pub async fn obs_websocket(
    // e_sender: UnboundedSender<EventHandler>,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
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

            match run_ads(token_sender.clone()).await {
                Ok(retry) => {
                    println!("retry after {} seconds", retry);
                    println!("Starting a 90 seconds commercial break");

                    // TODO: send this to IRC
                    // ws_sender
                    //     .send(Message::Text(to_irc_message(
                    //         "Starting a 90 seconds commercial break",
                    //     )))
                    //     .await?;
                }
                Err(e) => {
                    if e.to_string().contains("too many requests") {
                        println!("{e:?}");
                    } else {
                        panic!("{e}");
                    }
                }
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
