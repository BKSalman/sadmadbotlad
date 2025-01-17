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
    let Ok(client) = Client::connect("localhost", 4455, Some(&api_info.obs_server_password)).await
    else {
        tracing::error!("Could not connect to obs websocket");
        return Ok(());
    };

    if let Err(e) = refresh_alert_box(&client).await {
        tracing::error!("{e}");
    }

    let events = client.events()?;
    pin_mut!(events);

    while let Some(event) = events.next().await {
        if let Event::CurrentProgramSceneChanged { id } = event {
            if let Err(e) = refresh_alert_box(&client).await {
                tracing::error!("{e}");
            }

            if id.name != "Random" {
                continue;
            }

            match run_ads(token_sender.clone()).await {
                Ok(retry) => {
                    tracing::info!("retry after {} seconds", retry);
                    tracing::info!("Starting a 90 seconds commercial break");

                    // TODO: send this to IRC
                    // ws_sender
                    //     .send(Message::Text(to_irc_message(
                    //         "Starting a 90 seconds commercial break",
                    //     )))
                    //     .await?;
                }
                Err(e) => {
                    if e.to_string().contains("too many requests") {
                        tracing::error!("{e:?}");
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
            scene: current_scene.id.clone().into(),
            source: "AlertBox",
            search_offset: None,
        })
        .await?;

    client
        .scene_items()
        .set_enabled(obws::requests::scene_items::SetEnabled {
            scene: current_scene.id.clone().into(),
            item_id: id,
            enabled: false,
        })
        .await?;

    tokio::time::sleep(Duration::from_millis(500)).await;

    client
        .scene_items()
        .set_enabled(obws::requests::scene_items::SetEnabled {
            scene: current_scene.id.into(),
            item_id: id,
            enabled: true,
        })
        .await?;

    Ok(())
}
