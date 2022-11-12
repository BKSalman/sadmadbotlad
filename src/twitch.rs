#![allow(unused)]
use axum::{http, response::IntoResponse, Extension};
use eyre::WrapErr;
use futures_util::TryStreamExt;
use hyper::StatusCode;
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::RwLock;
use twitch_api::{
    eventsub::{
        stream::{StreamOnlineV1, StreamOnlineV1Payload},
        Event, EventType, Status,
    },
    helix::{
        self,
        eventsub::{
            CreateEventSubSubscriptionBody, CreateEventSubSubscriptionRequest,
            GetEventSubSubscriptionsRequest,
        },
    },
    HelixClient,
};
use twitch_oauth2::AppAccessToken;

use crate::{ApiInfo, Config};

pub async fn eventsub(
    Extension(config): Extension<Arc<Config>>,
    Extension(api_info): Extension<Arc<ApiInfo>>,
    request: http::Request<axum::body::Body>,
) -> impl IntoResponse {
    let (parts, body) = request.into_parts();

    let body = hyper::body::to_bytes(body).await.unwrap();

    let request = http::Request::from_parts(parts, &body);

    println!("got event {}", std::str::from_utf8(&body).unwrap());
    println!("got event headers {:?}", request.headers());

    if !Event::verify_payload(&request, api_info.secret()) {
        return (StatusCode::BAD_REQUEST, "Invalid signature".to_string());
    }

    if let Some(id) = request.headers().get("Twitch-Eventsub-Message-Id") {
        println!("got already seen event");
        return (StatusCode::OK, "".to_string());
    }

    let event = Event::parse_http(&request).unwrap();

    if let Some(ver) = event.get_verification_request() {
        println!("subscription was verified");
        return (StatusCode::OK, ver.challenge.clone());
    }

    if event.is_revocation() {
        println!("subscription was revoked");
        return (StatusCode::OK, "".to_string());
    }

    use twitch_api::eventsub::{Message, Payload};

    match event {
        Event::StreamOnlineV1(Payload {
            message:
                Message::Notification(StreamOnlineV1Payload {
                    broadcaster_user_id,
                    broadcaster_user_name,
                    started_at,
                    ..
                }),
            ..
        }) => {
            println!("{broadcaster_user_name} is streaming");
        }
        _ => {}
    }

    (StatusCode::OK, String::default())
}

pub async fn eventsub_register(
    token: Arc<RwLock<AppAccessToken>>,
    config: Arc<Config>,
    client: HelixClient<'static, reqwest::Client>,
    website: String,
    sign_secret: String,
) -> eyre::Result<()> {

    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // check every day
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(24 * 60 * 60));

    loop {
        // check if already registered
        interval.tick().await;

        println!("checking subs");

        let req = GetEventSubSubscriptionsRequest::status(Status::Enabled);

        let subs = helix::make_stream(req, &*token.read().await, &client, |res| {
            VecDeque::from(res.subscriptions)
        })
        .try_collect::<Vec<_>>()
        .await?;

        let online_exists = subs.iter().any(|sub| {
            sub.transport.callback == website
                && sub.type_ == EventType::StreamOnline
                && sub.version == "1"
                && sub
                    .condition
                    .as_object()
                    .expect("a stream.online event did not contain broadcaster")
                    .get("broadcaster_user_id")
                    .unwrap()
                    .as_str()
                    == Some(config.broadcaster.id.as_str())
        });

        println!("got existing subs");

        drop(subs);

        if !online_exists {
            let request = CreateEventSubSubscriptionRequest::new();

            let body = CreateEventSubSubscriptionBody::new(
                StreamOnlineV1::broadcaster_user_id(config.broadcaster.id.clone()),
                twitch_api::eventsub::Transport::webhook(
                    website.clone(),
                    sign_secret.clone(),
                ),
            );

            client
                .req_post(request, body, &*token.read().await)
                .await
                .wrap_err_with(|| "when registering online event")?;
        }
    }
}
