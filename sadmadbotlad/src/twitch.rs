use std::{
    fs,
    net::{Ipv4Addr, SocketAddrV4},
};

use chrono::{Duration, Local};
use eyre::Context;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{
    net::TcpListener,
    sync::{mpsc, oneshot},
};
use tokio_tungstenite::{accept_async, tungstenite};

use crate::ApiInfo;

#[allow(unused)]
#[derive(Deserialize)]
pub struct TwitchApiResponse {
    pub data: Vec<TwitchChannelInfo>,
    pagination: Option<Pagination>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Pagination {
    cursor: Option<String>,
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

#[derive(Deserialize, Debug)]
pub struct TwitchAd {
    pub data: Vec<AdData>,
}

#[derive(Deserialize, Debug)]
pub struct AdData {
    pub length: i32,
    pub message: String,
    pub retry_after: i64,
}

pub async fn set_title(
    new_title: &str,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
) -> Result<(), eyre::Report> {
    // request twitch patch change title

    let http_client = Client::new();

    let (one_shot_sender, one_shot_receiver) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(one_shot_sender))?;

    let Ok(api_info) = one_shot_receiver.await else {
        return Err(eyre::eyre!("Failed to get token"));
    };

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
        return Err(eyre::eyre!("set_title:: Unauthorized"));
    }

    Ok(())
}

pub async fn get_title(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
) -> Result<String, eyre::Report> {
    let http_client = Client::new();

    let (one_shot_sender, one_shot_receiver) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(one_shot_sender))?;

    let Ok(api_info) = one_shot_receiver.await else {
        return Err(eyre::eyre!("Failed to get token"));
    };

    let res = http_client
        .get("https://api.twitch.tv/helix/channels?broadcaster_id=143306668")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .send()
        .await?;

    if res.status() == StatusCode::UNAUTHORIZED {
        return Err(eyre::eyre!("get_title:: Unauthorized"));
    }

    let res = res.json::<Value>().await?;

    Ok(res["data"][0]["title"].as_str().unwrap().to_string())
}

pub async fn get_access_token_from_code(code: &str) -> Result<(), eyre::Report> {
    let config_str = fs::read_to_string("config.toml").wrap_err_with(|| "no config file")?;

    let mut configs =
        toml::from_str::<ApiInfo>(&config_str).wrap_err_with(|| "couldn't parse configs")?;

    let http_client = Client::new();

    let res = http_client
        .post("https://id.twitch.tv/oauth2/token")
        .form(&json!({
            "client_id": configs.twitch.client_id,
            "client_secret": configs.twitch.client_secret,
            "code": code,
            "grant_type": "authorization_code",
            "redirect_uri": "http://localhost:8080/code",
        }))
        .send()
        .await?;

    if !res.status().is_success() {
        return Err(eyre::eyre!(
            "get access token:: {}::{}",
            res.status(),
            res.text().await?
        ));
    }

    let res = res.json::<Value>().await?;

    println!("{:#?}\r", res["scope"].as_array().unwrap());

    configs.twitch.twitch_refresh_token = res["refresh_token"].as_str().unwrap().to_string();
    configs.twitch.twitch_access_token = res["access_token"].as_str().unwrap().to_string();

    fs::write(
        "config.toml",
        toml::to_string(&configs).expect("parse api info struct to string"),
    )?;

    Ok(())
}

pub async fn get_user_id(
    login_name: impl Into<String>,
    api_info: &TwitchApiInfo,
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
    api_info: &TwitchApiInfo,
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
        Some(id) => user_id == id,
        None => false,
    })
}

pub async fn run_ads(
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
) -> eyre::Result<i64> {
    let http_client = Client::new();

    let (one_shot_sender, one_shot_receiver) = oneshot::channel();

    token_sender.send(TwitchTokenMessages::GetToken(one_shot_sender))?;

    let Ok(api_info) = one_shot_receiver.await else {
        return Err(eyre::eyre!("Failed to get token"));
    };

    let res = http_client
        .post("https://api.twitch.tv/helix/channels/commercial")
        .bearer_auth(api_info.twitch_access_token.clone())
        .header("Client-Id", api_info.client_id.clone())
        .json(&json!({
            "broadcaster_id": "143306668",
            "length": "90",
        }))
        .send()
        .await?;

    if !res.status().is_success() {
        return Err(eyre::eyre!(
            "{} :: message: {}",
            res.status(),
            res.text().await?
        ));
    }

    let res = res.json::<TwitchAd>().await?;

    println!("{:#?}", res);

    Ok(res.data[0].retry_after)
}

pub async fn access_token() -> eyre::Result<()> {
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

// TODO: no need to store all of these in this struct
// make it ONLY for twitch token
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TwitchApiInfo {
    pub user: String,
    pub client_id: String,
    pub client_secret: String,
    pub twitch_access_token: String,
    pub twitch_refresh_token: String,
    #[serde(default)]
    pub expires_at: chrono::NaiveDateTime,
}

pub struct TwitchToken {
    receiver: mpsc::UnboundedReceiver<TwitchTokenMessages>,
    client: reqwest::Client,
    api_info: TwitchApiInfo,
}

#[derive(Debug)]
pub enum TwitchTokenMessages {
    GetToken(oneshot::Sender<TwitchApiInfo>),
}

impl TwitchToken {
    pub fn new(
        api_info: TwitchApiInfo,
        receiver: mpsc::UnboundedReceiver<TwitchTokenMessages>,
    ) -> Self {
        let client = reqwest::Client::new();
        Self {
            client,
            api_info,
            receiver,
        }
    }

    pub async fn handle_messages(mut self) -> eyre::Result<()> {
        while let Some(message) = self.receiver.recv().await {
            match message {
                TwitchTokenMessages::GetToken(response) => {
                    self.update_token().await?;

                    response
                        .send(self.api_info.clone())
                        .expect("Could not send token");
                }
            }
        }
        Ok(())
    }

    pub async fn update_token(&mut self) -> eyre::Result<()> {
        let now =
            chrono::NaiveDateTime::from_timestamp_millis(Local::now().timestamp_millis()).unwrap();

        if self.api_info.expires_at > now {
            return Ok(());
        }

        let res = self
            .client
            .get("https://id.twitch.tv/oauth2/validate")
            .header(
                "Authorization",
                format!("OAuth {}", self.api_info.twitch_access_token),
            )
            .send()
            .await?;

        if !res.status().is_success() {
            println!("token expired. Refreshing...");
            self.refresh_access_token().await?;
        }

        self.api_info.expires_at = now.checked_add_signed(Duration::seconds(3600)).unwrap();

        Ok(())
    }

    pub async fn refresh_access_token(&mut self) -> Result<(), eyre::Report> {
        println!("Refreshing token");
        let http_client = Client::new();

        let res = http_client
            .post("https://id.twitch.tv/oauth2/token")
            .json(&json!({
                "client_id": self.api_info.client_id,
                "client_secret": self.api_info.client_secret,
                "grant_type": "refresh_token",
                "refresh_token": self.api_info.twitch_refresh_token,
            }))
            .send()
            .await?;

        if res.status() == StatusCode::UNAUTHORIZED {
            return Err(eyre::eyre!("Unauthorized:: could not refresh access token"));
        }

        let res = res.json::<Value>().await?;

        let config_str = fs::read_to_string("config.toml").wrap_err_with(|| "no config file")?;

        let mut configs =
            toml::from_str::<ApiInfo>(&config_str).wrap_err_with(|| "couldn't parse configs")?;

        configs.twitch.twitch_refresh_token = res["refresh_token"].as_str().unwrap().to_string();
        configs.twitch.twitch_access_token = res["access_token"].as_str().unwrap().to_string();

        fs::write(
            "config.toml",
            toml::to_string(&configs).expect("parse api info struct to string"),
        )?;

        self.api_info.twitch_refresh_token = configs.twitch.twitch_refresh_token;
        self.api_info.twitch_access_token = configs.twitch.twitch_access_token;

        Ok(())
    }
}
