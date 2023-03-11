use serde::Serialize;
use serde_json::Value;

use crate::ApiInfo;

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

pub async fn online_notification(
    title: &str,
    game_name: &str,
    timestamp: i64,
    api_info: &ApiInfo,
) -> Result<String, eyre::Report> {
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

    let http_client = reqwest::Client::new();

    let res = http_client
        .post("https://discordapp.com/api/channels/575540932028530699/messages")
        .header("authorization", format!("Bot {}", api_info.discord_token))
        // .bearer_auth(format!("Bot {}", discord_token))
        .json(&message)
        .send()
        .await?
        .json::<Value>()
        .await?;

    Ok(res["id"].as_str().expect("message id").to_string())
}

pub async fn offline_notification(
    title: &str,
    game_name: &str,
    api_info: &ApiInfo,
    message_id: &str,
) -> eyre::Result<()> {
    let timestamp = chrono::offset::Local::now().timestamp();

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
                    name: String::from("Ended"),
                    value: format!("<t:{timestamp}:R>"), //<t:1668640560:R>
                    inline: true,
                }
            ],
        }],
    };

    let http_client = reqwest::Client::new();

    http_client
        .patch(format!(
            "https://discordapp.com/api/channels/575540932028530699/messages/{message_id}"
        ))
        .header("authorization", format!("Bot {}", api_info.discord_token))
        .json(&message)
        .send()
        .await?
        .text()
        .await?;

    Ok(())
}
