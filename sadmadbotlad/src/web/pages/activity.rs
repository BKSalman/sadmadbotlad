use topcoat::{
    asset::asset,
    context::{Cx, app_context},
    router::page,
    runtime::{Event, procedure, shard},
    view::view,
};

use crate::{
    Alert, AlertEventType,
    db::{DBMessage, DbEventRecord},
    web::AppContext,
};

/// Newest-first cap on the feed, so the page does not grow without bound.
const FEED_LIMIT: usize = 100;

/// `OneShotReceiver::recv` is `std::sync::mpsc`, so it blocks; keep it off the
/// runtime threads.
async fn db_events(cx: &Cx) -> anyhow::Result<Vec<DbEventRecord>> {
    let db_tx = app_context::<AppContext>(cx).db_tx.clone();
    let (tx, rx) = crate::oneshot();
    db_tx.send(DBMessage::GetEvents(tx))?;

    Ok(tokio::task::spawn_blocking(move || rx.recv()).await??)
}

async fn db_event(cx: &Cx, id: i32) -> anyhow::Result<Option<DbEventRecord>> {
    let db_tx = app_context::<AppContext>(cx).db_tx.clone();
    let (tx, rx) = crate::oneshot();
    db_tx.send(DBMessage::GetEvent(id, tx))?;

    Ok(tokio::task::spawn_blocking(move || rx.recv()).await??)
}

fn describe(alert_type: &AlertEventType) -> String {
    match alert_type {
        AlertEventType::Follow { follower } => format!("{follower} followed"),
        AlertEventType::Raid { from, viewers } => {
            format!("{from} raided with {viewers} viewers")
        }
        AlertEventType::Subscribe { subscriber, tier } => {
            format!("{subscriber} subscribed tier {tier}")
        }
        AlertEventType::GiftSub {
            gifter,
            total,
            tier,
        } => format!("{gifter} gifted {total} tier {tier} subs"),
        AlertEventType::ReSubscribe {
            subscriber,
            tier,
            subscribed_for: _,
            streak,
        } => {
            if *streak > 1 {
                format!("{subscriber} resubscribed {streak} months streak")
            } else {
                format!("{subscriber} resubscribed tier {tier}")
            }
        }
        AlertEventType::GiftedSub { gifted, tier } => {
            format!("{gifted} got gifted a tier {tier} sub")
        }
        AlertEventType::Bits {
            is_anonymous,
            cheerer,
            bits,
            message: _,
        } => {
            if *is_anonymous {
                format!("Anonymous cheered {bits} bits")
            } else {
                format!("{cheerer} cheered {bits} bits")
            }
        }
    }
}

/// Pushes a stored event back onto the alerts channel, so the `/alerts` overlay
/// plays it again. This is a public endpoint: `id` is attacker-controlled and a
/// miss must stay a no-op.
#[procedure]
async fn replay_event(cx: &Cx, id: String) -> topcoat::Result<()> {
    let Ok(id) = id.parse::<i32>() else {
        return Ok(());
    };

    let Some(record) = db_event(cx, id).await? else {
        return Ok(());
    };

    let alerts_sender = app_context::<AppContext>(cx).alerts_sender.clone();
    let _ = alerts_sender.send(Alert {
        new: true,
        r#type: record.alert_type,
    });

    Ok(())
}

/// Re-runs whenever `bump` changes, which activity.js does on every websocket
/// message. The argument's value is unused; it only defeats the shard's
/// argument dedup.
#[shard]
async fn feed_shard(cx: &Cx, bump: String) -> topcoat::Result {
    let _ = bump;

    let mut records = db_events(cx).await?;
    records.reverse();
    records.truncate(FEED_LIMIT);

    // The id rides on the button's `value` rather than being captured into the
    // handler: a captured external would be borrowed by the generated closure
    // and outlive it.
    let rows = records
        .into_iter()
        .map(|record| (record.id.to_string(), describe(&record.alert_type)))
        .collect::<Vec<_>>();

    view! {
        <div class="mx-auto flex w-[70vw] flex-col items-center gap-[5px]">
            if rows.is_empty() {
                <p class="py-8 text-white/60">"No events yet"</p>
            }

            for (id, text) in rows {
                <div class="flex w-full items-center justify-between bg-[#2c2e2f] px-4 py-2">
                    <span class="text-white">(text)</span>

                    <button
                        class="cursor-pointer px-2 text-white"
                        title="Replay"
                        value=(id)
                        @click=$(async |e: Event| {
                            replay_event(e.target.value).await;
                        })
                    >
                        "↻"
                    </button>
                </div>
            }
        </div>
    }
}

#[page("/activity")]
async fn activity() -> topcoat::Result {
    view! {
        <!DOCTYPE html>
        <html>
            <head>
                topcoat::dev::script()
                topcoat::runtime::script()
                <link rel="stylesheet" href=(topcoat::tailwind::stylesheet!())>
                <script type="module" src=(asset!("./activity.js"))></script>
            </head>
            <body class="min-h-screen bg-[rgb(24,26,27)] py-4 font-bold">
                signal bump = String::new();

                <input
                    type="hidden"
                    id="activity-bus"
                    @input=$(|e: Event| bump.set(e.target.value))
                >

                feed_shard(bump: $(bump.get()))
            </body>
        </html>
    }
}
