use std::{
    fs,
    io::Read
};

use eyre::Context;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use twitch_api::types::Timestamp;

use crate::ApiInfo;

#[derive(Debug, Deserialize)]
pub struct WsEventSub {
    pub metadata: WsMetaData,
    pub payload: WsPayload,
}

#[allow(unused)]
#[derive(Debug, Deserialize)]
pub struct WsMetaData {
    message_id: String,
    message_type: String,
    message_timestamp: Timestamp,
    subscription_type: Option<String>,
    subscription_version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WsPayload {
    pub session: Option<WsSession>,
    pub subscription: Option<WsSubscription>,
    pub event: Option<WsEvent>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WsSubscription {
    pub id: String,
    pub status: String,
    pub r#type: String,
    version: String,
    condition: WsCondition,
    transport: WsTransport,
    created_at: Timestamp,
    cost: u8,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WsCondition {
    broadcaster_user_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WsTransport {
    method: String,
    session_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WsSession {
    pub id: String,
    pub status: String,
    pub connected_at: Timestamp,
    pub keepalive_timeout_seconds: Option<u32>,
    pub reconnect_url: Option<String>,
}

#[allow(unused)]
#[derive(Debug, Deserialize)]
pub struct WsEvent {
    user_id: Option<String>,
    pub user_login: Option<String>,
    pub user_name: Option<String>,
    broadcaster_user_id: String,
    broadcaster_user_login: String,
    broadcaster_user_name: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EventSub {
    r#type: String,
    version: u8,
    condition: EventSubCondition,
    transport: WsTransport,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EventSubCondition {
    broadcaster_user_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EventSubResponse {
    pub data: Vec<WsSubscription>,
    total_cost: u16,
    max_total_cost: u32,
    pagination: Pagination,
}

#[derive(Debug, Deserialize, Serialize)]
struct Pagination {
    cursor: Option<String>,
}

#[allow(unused)]
#[derive(Deserialize)]
pub struct TwitchApiResponse {
    pub data: Vec<TwitchChannelInfo>,
    pagination: Option<Pagination>,
}

#[allow(unused)]
#[derive(Deserialize)]
pub struct TwitchChannelInfo {
    id: String,
    pub user_id: String,
    pub title: String,
    pub game_name: String,
    pub started_at: String,

    user_login: String,
    user_name: String,
    r#type: String,
    viewer_count: u32,
    game_id: String,
    language: String,
    thumbnail_url: String,
    tag_ids: Option<Vec<String>>,
    is_mature: bool,
}

#[allow(unused)]
#[derive(Deserialize)]
pub struct GetChannelInfo {
    pub data: Vec<TwitchChannelInfo>,
}

#[allow(unused)]
#[derive(Deserialize)]
pub struct TwitchGetChannelInfo {
    pub broadcaster_id: String,
    pub title: String,
    pub game_name: String,

    broadcaster_login: String,
    broadcaster_name: String,
    game_id: String,
    broadcaster_language: String,
}

pub async fn set_title(new_title: &str, api_info: &mut ApiInfo) -> Result<(), eyre::Report> {
    // request twitch patch change title

    let http_client = Client::new();

    let res = http_client
        .patch("https://api.twitch.tv/helix/channels?broadcaster_id=143306668")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .json(&json!({
            "title": new_title,
        }))
        .send()
        .await?;

    if res.status() == StatusCode::UNAUTHORIZED {
        refresh_access_token(api_info).await?;
    }

    Ok(())
}

pub async fn get_title(api_info: &mut ApiInfo) -> Result<String, eyre::Report> {
    let http_client = Client::new();

    let res = http_client
        .get("https://api.twitch.tv/helix/channels?broadcaster_id=143306668")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .send()
        .await?;

    if res.status() == StatusCode::UNAUTHORIZED {
        refresh_access_token(api_info).await?;
    }

    let res = res.json::<Value>().await?;

    Ok(res["data"][0]["title"].as_str().unwrap().to_string())
}

pub async fn refresh_access_token(api_info: &mut ApiInfo) -> Result<(), eyre::Report> {
    println!("Refreshing token");
    let http_client = Client::new();

    let res = http_client
        .post("https://id.twitch.tv/oauth2/token")
        .json(&json!({
            "client_id": api_info.client_id,
            "client_secret": api_info.client_secret,
            "grant_type": "refresh_token",
            "refresh_token": api_info.twitch_refresh_token,
        }))
        .send()
        .await?;

    if res.status() == StatusCode::UNAUTHORIZED {
        panic!("Unauthorized:: could not refresh access token")
    }

    let res = res.json::<Value>().await?;

    let mut config_str = String::new();

    let mut config_file = fs::File::open("config.toml").wrap_err_with(|| "no config file")?;

    config_file
        .read_to_string(&mut config_str)
        .wrap_err_with(|| "couldn't read config file")?;

    let mut configs =
        toml::from_str::<ApiInfo>(&config_str).wrap_err_with(|| "couldn't parse configs")?;

    configs.twitch_refresh_token = res["refresh_token"].as_str().unwrap().to_string();
    configs.twitch_access_token = res["access_token"].as_str().unwrap().to_string();

    fs::write("config.toml", toml::to_string(&configs).expect("parse api info struct to string"))?;
    
    api_info.twitch_refresh_token = configs.twitch_refresh_token;
    api_info.twitch_access_token = configs.twitch_access_token;

    Ok(())
}

pub async fn get_user_id(
    login_name: impl Into<String>,
    api_info: &ApiInfo,
) -> Result<String, eyre::Report> {
    let http_client = reqwest::Client::new();

    let res = http_client
        .get(format!(
            "https://api.twitch.tv/helix/users?login={}",
            login_name.into()
        ))
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .send()
        .await?
        .json::<Value>()
        .await?;

    Ok(res["data"][0]["id"].as_str().expect("user id").to_string())
}

pub async fn is_vip(
    login_name: impl Into<String>,
    api_info: &ApiInfo,
) -> Result<bool, eyre::Report> {
    let http_client = reqwest::Client::new();
    let user_id = get_user_id(login_name, api_info).await?;

    let res = http_client
        .get(format!(
            "https://api.twitch.tv/helix/channels/vips?broadcaster_id=143306668&user_id={}&first=1",
            user_id
        ))
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .send()
        .await?
        .json::<Value>()
        .await?;

    Ok(match res["data"][0]["user_id"].as_str() {
        Some(id) => {
            if user_id == id {
                true
            } else {
                false
            }
        }
        None => false,
    })
}
