use std::sync::Arc;

use crate::ApiInfo;
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use serde_json::Value;

const FRAGMENT: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'<').add(b'>').add(b'`');

pub struct VideoInfo {
    pub id: String,
    pub title: String,
}

pub fn video_id_from_url(url: &str) -> Result<&str, eyre::Report> {
    if url.contains("?v=") && url.contains('&') {
        return Ok(&url[url.find("?v=").unwrap() + 3..url.find('&').unwrap()]);
    } else if url.contains("?v=") {
        return Ok(&url[url.find("?v=").unwrap() + 3..]);
    } else if url.contains("/watch/") || url.contains("https://youtu.be") {
        return Ok(url.rsplit_once('/').expect("yt watch format link").1);
    }

    Err(eyre::eyre!("Not A Valid Youtube URL"))
}

pub async fn video_title(video_id: &str, api_info: Arc<ApiInfo>) -> Result<String, eyre::Report> {
    let http_client = reqwest::Client::new();

    let res = http_client
        .get(format!(
            "https://youtube.googleapis.com/youtube/v3/\
                                    search?part=snippet&maxResults=1&q={}&type=video&key={}",
            video_id, api_info.google_api_key
        ))
        .send()
        .await?;

    if !res.status().is_success() {
        return Err(eyre::eyre!(
            "video_title:: {}: {}",
            res.status(),
            res.text().await?
        ));
    }

    let res = res.json::<Value>().await?;

    if let Some(video_title) = res["items"][0]["snippet"]["title"].as_str() {
        Ok(video_title.to_owned())
    } else {
        Err(eyre::eyre!("Failed to get video title"))
    }
}

// TODO: move this to be a method of YoutubeApiInfo or something
pub async fn video_info(
    video_query: &str,
    api_info: Arc<ApiInfo>,
) -> Result<VideoInfo, eyre::Report> {
    let http_client = reqwest::Client::new();

    tracing::debug!("youtube video query: {video_query}");

    let res = http_client
        .get(format!(
            "https://youtube.googleapis.com/youtube/v3/\
                                    search?part=snippet&maxResults=1&q={}&type=video&key={}",
            utf8_percent_encode(video_query, FRAGMENT),
            api_info.google_api_key
        ))
        .send()
        .await?;

    if !res.status().is_success() {
        return Err(eyre::eyre!(
            "{} :: message: {}",
            res.status(),
            res.text().await?
        ));
    }

    let res = res.json::<Value>().await?;

    let video_title = res["items"][0]["snippet"]["title"]
        .as_str()
        .expect("yt video title");

    let video_id = res["items"][0]["id"]["videoId"]
        .as_str()
        .expect("yt video id");

    Ok(VideoInfo {
        id: video_id.to_string(),
        title: video_title.to_string(),
    })
}
