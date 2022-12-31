use reqwest::StatusCode;
use serde::Serialize;

use crate::{twitch::TwitchApiResponse, ApiInfo};

#[derive(Serialize)]
struct Image {
    url: String,
}

#[derive(Serialize)]
struct Embed {
    title: String,
    author: Author,
    url: String,
    image: Image,
    fields: Vec<Field>,
}

#[derive(Serialize)]
struct Author {
    name: String,
    icon_url: String,
}

#[derive(Serialize)]
struct Field {
    name: String,
    value: String,
    inline: bool,
}

#[derive(Serialize)]
struct Message {
    content: String,
    embeds: Vec<Embed>,
}

pub async fn online_notification(api_info: &ApiInfo) -> Result<(), eyre::Report> {
    let http_client = reqwest::Client::new();

    let res = http_client
        .get("https://api.twitch.tv/helix/streams?user_login=sadmadladsalman")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .send()
        .await?;
    if res.status() == StatusCode::UNAUTHORIZED {
        return Err(eyre::eyre!("Unauthorized"));
    }
    let res = res.json::<TwitchApiResponse>().await?;

    let timestamp = chrono::DateTime::parse_from_rfc3339(&res.data[0].started_at)?.timestamp();

    let title = &res.data[0].title;

    let game_name = &res.data[0].game_name;

    let message = Message {
        content: String::from("<@&897124518374559794> Salman is streaming\n"),
        embeds: vec![Embed {
            title: title.to_string(),
            author: Author {
                name: String::from("SadMadLadSalMaN"),
                icon_url: String::from("https://static-cdn.jtvnw.net/jtv_user_pictures/497627b8-c550-4703-ae00-46a4a3cdc4c8-profile_image-300x300.png"),
            },
            url: String::from("https://www.twitch.tv/sadmadladsalman"),
            image: Image {
                url: format!("https://static-cdn.jtvnw.net/previews-ttv/live_user_sadmadladsalman-320x180.jpg?something={}", timestamp)
            },
            fields: vec![
                Field {
                    name: String::from("Game"),
                    value: game_name.to_string(),
                    inline: true,
                },
                Field {
                    name: String::from("Started"),
                    value: format!("<t:{timestamp}:R>"), //<t:1668640560:R>
                    inline: true,
                }
            ],
        }],
    };

    http_client
        .post("https://discordapp.com/api/channels/575540932028530699/messages")
        .header("authorization", format!("Bot {}", api_info.discord_token))
        // .bearer_auth(format!("Bot {}", discord_token))
        .json(&message)
        .send()
        .await?
        .text()
        .await?;

    Ok(())
}

// TODO: updating the notification message to show that I ended the stream might be cool

// pub async fn offline_notification(api_info: &ApiInfo, title: &str, game_name: &str) -> Result<(), reqwest::Error> {
//     let http_client = reqwest::Client::new();

//     let message = Message {
//         content: String::from("<@&897124518374559794> Salman is streaming\n"),
//         embeds: vec![Embed {
//             title: format!("{}", title),
//             author: Author {
//                 name: String::from("SadMadLadSalMaN"),
//                 icon_url: String::from("https://static-cdn.jtvnw.net/jtv_user_pictures/497627b8-c550-4703-ae00-46a4a3cdc4c8-profile_image-300x300.png"),
//             },
//             url: String::from("https://www.twitch.tv/sadmadladsalman"),
//             image: Image {
//                 url: format!("https://static-cdn.jtvnw.net/previews-ttv/live_user_sadmadladsalman-320x180.jpg?something={}", timestamp)
//             },
//             fields: vec![
//                 Field {
//                     name: String::from("Game"),
//                     value: game_name.to_string(),
//                     inline: true,
//                 },
//                 Field {
//                     name: String::from("Started"),
//                     value: format!("<t:{timestamp}:R>"), //<t:1668640560:R>
//                     inline: true,
//                 }
//             ],
//         }],
//     };

//     http_client
//         .post("https://discordapp.com/api/channels/575540932028530699/messages")
//         .header("authorization", format!("Bot {}", api_info.discord_token))
//         // .bearer_auth(format!("Bot {}", discord_token))
//         .json(&message)
//         .send()
//         .await?
//         .text()
//         .await?;

//     Ok(())
// }
