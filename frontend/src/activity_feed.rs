use futures::{channel::mpsc::Sender, stream::SplitStream, SinkExt, StreamExt};
use gloo::console;
use gloo_net::websocket::{futures::WebSocket, Message};
use serde_json::Value;
use wasm_bindgen_futures::spawn_local;
use yew::{html::Scope, prelude::*};

use crate::{components::event::Event, Alert, AlertEventType};

pub enum Msg {
    Event(Alert),
    ReplayEvent(Alert),
    Db(String),
    Nothing,
}

pub struct Activity {
    sender: Sender<Message>,
    alerts: Vec<Alert>,
}

impl Component for Activity {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let ws = WebSocket::open("ws://localhost:4000").expect("Ws");

        let (mut ws_sender, ws_receiver) = ws.split();

        let (sender, mut in_rx) = futures::channel::mpsc::channel::<Message>(1000);

        spawn_local(async move {
            while let Some(msg) = in_rx.next().await {
                ws_sender.send(msg).await.expect("send");
            }
        });

        let mut senderc = sender.clone();
        spawn_local(async move {
            senderc
                .send(Message::Text(String::from("db")))
                .await
                .expect("send");
        });

        let scope = ctx.link().clone();

        spawn_local(async move {
            handle_alert(ws_receiver, scope).await;
        });

        Self {
            sender,
            alerts: Vec::new(),
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::ReplayEvent(event) => {
                let mut senderc = self.sender.clone();
                spawn_local(async move {
                    let alert = serde_json::to_string(&event).expect("alert");
                    senderc
                        .send(Message::Text(alert))
                        .await
                        .expect("send alert");
                });
                true
            }
            Msg::Event(mut alert) => {
                if !alert.new {
                    return false;
                }

                alert.new = false;
                self.alerts.push(alert);
                console::log!(serde_json::to_string(&self.alerts).expect("events"));
                true
            }
            Msg::Db(db) => {
                let (_, db) = db.split_once("::").expect("split db");
                let db_alerts: Vec<Value> = serde_json::from_str(&db).expect("db alerts");
                let db_alerts: Vec<Alert> = db_alerts
                    .into_iter()
                    .map(|e| match e["type"].as_str().expect("alert type") {
                        "Follow" => {
                            let follower = e["follower"].as_str().expect("follower").to_string();
                            Alert {
                                new: false,
                                r#type: AlertEventType::Follow { follower },
                            }
                        }
                        "Subscribe" => {
                            let subscriber =
                                e["subscriber"].as_str().expect("subscriber").to_string();
                            let tier = e["tier"].as_str().expect("tier").to_string();

                            Alert {
                                new: false,
                                r#type: AlertEventType::Subscribe { subscriber, tier },
                            }
                        }
                        "ReSubscribe" => {
                            let subscriber =
                                e["subscriber"].as_str().expect("subscriber").to_string();

                            let tier = e["tier"].as_str().expect("tier").to_string();

                            let subscribed_for =
                                e["subscribed_for"].as_u64().expect("subscribed for");

                            let streak = e["streak"].as_u64().expect("subscribed for");

                            Alert {
                                new: false,
                                r#type: AlertEventType::ReSubscribe {
                                    subscriber,
                                    tier,
                                    subscribed_for,
                                    streak,
                                },
                            }
                        }
                        "Raid" => {
                            let from = e["from"].as_str().expect("from").to_string();

                            let viewers = e["viewers"].as_u64().expect("viewers");

                            Alert {
                                new: false,
                                r#type: AlertEventType::Raid { from, viewers },
                            }
                        }
                        "Bits" => {
                            let cheerer = e["cheerer"].as_str().expect("from").to_string();

                            let message = e["message"].as_str().expect("from").to_string();

                            let is_anonymous = e["is_anonymous"].as_bool().expect("from");

                            let bits = e["viewers"].as_u64().expect("viewers");

                            Alert {
                                new: false,
                                r#type: AlertEventType::Bits {
                                    message,
                                    is_anonymous,
                                    cheerer,
                                    bits,
                                },
                            }
                        }
                        "GiftSub" => {
                            let gifter = e["gifter"].as_str().expect("gifter").to_string();

                            let total = e["message"].as_u64().expect("total");

                            let tier = e["tier"].as_str().expect("tier").to_string();

                            Alert {
                                new: false,
                                r#type: AlertEventType::GiftSub {
                                    gifter,
                                    total,
                                    tier,
                                },
                            }
                        }
                        "GiftedSub" => {
                            let gifted = e["gifted"].as_str().expect("gifted").to_string();

                            let tier = e["tier"].as_str().expect("tier").to_string();

                            Alert {
                                new: false,
                                r#type: AlertEventType::GiftedSub { tier, gifted },
                            }
                        }
                        _ => Alert::default(),
                    })
                    .collect();
                self.alerts.extend(db_alerts);
                true
            }
            Msg::Nothing => false,
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let cb = ctx.link().callback(|message: Msg| message);
        html! {
            <div class="event-list">
                {
                    self.alerts.clone()
                    .into_iter().rev().map(|s| {
                        let sc = s.clone();
                        let on_click = {
                            let cbc = cb.clone();
                            move |_| {cbc.emit(Msg::ReplayEvent(sc.clone()))}
                        };
                        match s.r#type {
                            AlertEventType::Follow { follower } => {
                                html! {
                                    < Event
                                        text={format!("{follower} followed")}
                                        on_click={on_click}
                                    />
                                }
                            },
                            AlertEventType::Raid { from, viewers } => {
                                html! {
                                    < Event
                                        text={format!("{from} raided with {viewers} viewers")}
                                        on_click={on_click}
                                    />
                                }
                            },
                            AlertEventType::Subscribe { subscriber, tier } => {
                                html! {
                                    < Event
                                        text={format!("{subscriber} subscribed tier {tier}")}
                                        on_click={on_click}
                                    />
                                }
                            },
                            AlertEventType::GiftSub { gifter, total, tier } => {
                                html! {
                                    < Event
                                        text={format!("{gifter} gifted {total} tier {tier} subs")}
                                        on_click={on_click}
                                    />
                                }
                            },
                            AlertEventType::ReSubscribe { subscriber, tier, subscribed_for: _, streak } => {
                                html! {
                                    < Event
                                        text={
                                                if streak > 1 {
                                                    format!("{subscriber} resubscribed {streak} months streak")
                                                } else {
                                                    format!("{subscriber} resubscribed tier {tier}")
                                                }
                                            }
                                        on_click={on_click}
                                    />
                                }
                            },
                            AlertEventType::GiftedSub { gifted, tier } => {
                                html! {
                                    < Event
                                        text={format!("{gifted} got gifted a tier {tier} sub")}
                                        on_click={on_click}
                                    />
                                }
                            },
                            AlertEventType::Bits { is_anonymous, cheerer, bits, ..} => {
                                let message = if is_anonymous {
                                    format!("Anonymous cheered {bits} bits")
                                } else {
                                    format!("{cheerer} cheered {bits} bits")
                                };
                                html! {
                                    < Event
                                        text={message}
                                        on_click={on_click}
                                    />
                                }
                            },
                            AlertEventType::Nothing => {html!{}},
                        }
                    }).collect::<Html>()
                }
            </div>
        }
    }
}

async fn handle_alert(mut ws_receiver: SplitStream<WebSocket>, scope: Scope<Activity>) {
    while let Some(ws_msg) = ws_receiver.next().await {
        match ws_msg {
            Ok(Message::Text(msg)) => {
                if msg.starts_with("db") {
                    scope.send_message(Msg::Db(msg));
                    return;
                }
                let alert = serde_json::from_str::<Alert>(&msg).expect("event");
                console::log!("got ", &msg);
                scope.send_message(Msg::Event(alert));
            }
            Ok(msg) => {
                console::log!(format!("{msg:?}"));
                scope.send_message(Msg::Nothing);
            }
            Err(e) => {
                console::log!(format!("{e:?}"));
                scope.send_message(Msg::Nothing);
            }
        }
    }
}
