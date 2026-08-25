use topcoat::{
    asset::asset,
    context::{Cx, app_context},
    router::{
        content::websocket::{self, WebSocketUpgrade},
        page, route,
    },
    runtime::{Event, procedure, shard},
    view::view,
};

use crate::{Alert, AlertEventType, web::AppContext};

#[procedure]
async fn send_alert(cx: &Cx) -> topcoat::Result<()> {
    let alerts_sender = app_context::<AppContext>(cx).alerts_sender.clone();

    let _ = alerts_sender.send(Alert::follow_test());

    Ok(())
}

#[page("/alerts")]
async fn alerts() -> topcoat::Result {
    view! {
        <!DOCTYPE html>
        <html>
            <head>
                topcoat::dev::script()
                topcoat::runtime::script()
                <link rel="stylesheet" href=(topcoat::tailwind::stylesheet!())>
                <script type="module" src=(asset!("./alerts.js"))></script>
            </head>
            <body>
                signal payload = String::new();

                <input
                    type="hidden"
                    id="alert-bus"
                    @input=$(|e: Event| payload.set(e.target.value))
                >

                alert_shard(alert_json: $(payload.get()))
            </body>
        </html>
    }
}

#[shard]
async fn alert_shard(alert_json: String) -> topcoat::Result {
    let payload = match alert_json.split_once(':') {
        Some((_seq, json)) => json,
        None => alert_json.as_str(),
    };

    let Ok(alert) = serde_json::from_str::<Alert>(payload) else {
        return view! { <div></div> };
    };
    println!("{alert:?}");

    let (video, message) = match alert.r#type {
        AlertEventType::Follow { follower } => {
            (String::from("follow"), format!("{follower} followed 😎!"))
        }
        AlertEventType::Raid { from, viewers } => (
            if from == "asd" {
                String::from("asd")
            } else {
                String::from("raid")
            },
            format!("{from} raided with {viewers} viewers 🦀!"),
        ),
        AlertEventType::Subscribe { subscriber, tier } => (
            String::from("sub"),
            format!("{subscriber} subscribed with tier {tier}!"),
        ),
        AlertEventType::GiftSub {
            gifter,
            total,
            tier,
        } => (
            String::from("sub"),
            if total > 1 {
                format!("{gifter} gifted {total} tier {tier} subs!")
            } else {
                format!("{gifter} gifted a tier {tier} sub!")
            },
        ),
        AlertEventType::ReSubscribe {
            subscriber,
            tier,
            subscribed_for: _,
            streak,
        } => (
            String::from("sub"),
            if streak > 1 {
                format!("{subscriber} resubscribed with a streak of {streak} months!!!")
            } else {
                format!("{subscriber} resubscribed with tier {tier}!")
            },
        ),
        AlertEventType::GiftedSub { gifted, tier } => (
            String::from("sub"),
            format!("{gifted} got gifted a tier {tier} sub!"),
        ),
        AlertEventType::Bits {
            is_anonymous,
            cheerer,
            bits,
            message,
        } => {
            if is_anonymous {
                (
                    String::from("cheer"),
                    format!("Anonymous cheered {bits} bits!\n{message}"),
                )
            } else {
                (
                    String::from("cheer"),
                    format!("{cheerer} cheered {bits} bits!\n{message}"),
                )
            }
        }
    };

    view! {
        <div class="flex h-screen w-screen flex-col items-center justify-center gap-4">
            <video
                class="min-h-0 w-full flex-1 object-contain"
                src=(format!("/assets/{video}.webm"))
                autoplay=(true)
                @ended="() => { const b = document.getElementById('alert-bus');
                                   b.value = ''; b.dispatchEvent(new Event('input')); }"
            ></video>
            <p class="shrink-0 px-4 pb-4 text-center text-5xl font-bold whitespace-pre-line
                      text-white [paint-order:stroke_fill] [-webkit-text-stroke:8px_#000]
                      [filter:drop-shadow(0_3px_10px_rgba(0,0,0,0.9))]">
                (message)
            </p>
        </div>
    }
}

#[route(GET "/alerts/ws")]
async fn alerts_ws(
    cx: &Cx,
    upgrade: WebSocketUpgrade,
) -> topcoat::Result<topcoat::router::response::Response> {
    let ctx = app_context::<AppContext>(cx);
    let mut alerts_receiver = ctx.alerts_sender.subscribe();
    upgrade.on_upgrade(move |mut socket| async move {
        while let Ok(received_alert) = alerts_receiver.recv().await {
            let alert_json = serde_json::to_string(&received_alert).expect("alert");

            socket
                .send(websocket::Message::Text(alert_json.into()))
                .await
                .expect("Sending alert json through websocket");
        }
    })
}
