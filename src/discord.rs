use reqwest;
use serde::Serialize;

#[derive(Serialize)]
struct Image {
    url: String,
}

#[derive(Serialize)]
struct Embed {
    title: String,
    url: String,
    image: Image,
}

#[derive(Serialize)]
struct Message {
    content: String,
    embeds: Vec<Embed>,
}

pub async fn send_notification() -> Result<(), reqwest::Error> {
    let http_client = reqwest::Client::new();

    let Ok(discord_token) = std::env::var("DISCORD_TOKEN") else {
        panic!("add DISCORD_TOKEN")
    };

    let message = Message {
        content: String::from("streaming :)"),
        embeds: vec![Embed {
            title: String::from("Watch stream"),
            url: String::from("https://www.twitch.tv/sadmadladsalman"),
            image: Image {
                url: String::from("https://static-cdn.jtvnw.net/previews-ttv/live_user_sadmadladsalman-320x180.jpg")
            }
        }],
    };

    http_client
        .post("https://discordapp.com/api/channels/575540932028530699/messages")
        .header("authorization", format!("Bot {}", discord_token))
        // .bearer_auth(format!("Bot {}", discord_token))
        .json(&message)
        .send()
        .await?
        .text()
        .await?;


    Ok(())
    
}
