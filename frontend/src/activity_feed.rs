use futures::{channel::mpsc::Sender, stream::SplitStream, SinkExt, StreamExt};
use gloo::console;
use gloo_net::websocket::{futures::WebSocket, Message};
use wasm_bindgen_futures::spawn_local;
use yew::{html::Scope, prelude::*};
use yew_icons::{Icon, IconId};

use crate::{AlertEventType, Alert};

pub enum Msg {
    Event(Alert),
    ReplayEvent(Alert),
    Nothing,
}

pub enum Tier {
    Tier1 = 1,
    Tier2 = 2,
    Tier3 = 3,
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
                    senderc.send(Message::Text(alert)).await.expect("send alert");
                });
                true
            }
            Msg::Event(mut alert) => {
                if !alert.new {
                    return false
                }

                alert.new = false;
                self.alerts.push(alert);
                console::log!(serde_json::to_string(&self.alerts).expect("events"));
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
                    .into_iter().map(|s| {
                        let sc = s.clone();
                        let cbc = cb.clone();
                        match s.alert_type {
                            AlertEventType::Follow { follower } => {
                                html_nested! {
                                    <div class="event">
                                        <div class="event-name">
                                            <span>{"Event: Follow"}</span>
                                        </div>
                                        <div class="event-args">
                                            <span>{"User: "}</span> <span>{ follower }</span>
                                        </div>
                                        <Icon class="replay-btn" onclick={move |_| {cbc.emit(Msg::ReplayEvent(sc.clone()))}} icon_id={IconId::FontAwesomeSolidReply}/>
                                    </div>
                                }
                            },
                            AlertEventType::Raid { from } => {
                                html_nested! {
                                    <div class="event">
                                        <div class="event-name">
                                            <span>{"Event: Raid"}</span>
                                        </div>
                                        <div class="event-args">
                                            <span>{"From: "}</span> <span>{ from }</span>
                                        </div>
                                        <Icon class="replay-btn" onclick={move |_| {cbc.emit(Msg::ReplayEvent(sc.clone()))}} icon_id={IconId::FontAwesomeSolidReply}/>
                                    </div>
                                }
                            },
                            AlertEventType::Subscribe { subscriber } => {
                                html_nested! {
                                    <div class="event">
                                        <div class="event-name">
                                            <span>{"Event: Subscribe"}</span>
                                        </div>
                                        <div class="event-args">
                                            <span>{"User: "}</span> <span>{ subscriber }</span>
                                        </div>
                                        <Icon class="replay-btn" onclick={move |_| {cbc.emit(Msg::ReplayEvent(sc.clone()))}} icon_id={IconId::FontAwesomeSolidReply}/>
                                    </div>
                                }
                            },
                        }
                    }).collect::<Html>()
                }
            </div>
        }
    }
}

async fn handle_alert(mut ws_receiver: SplitStream<WebSocket>, scope: Scope<Activity>) -> Msg {
    while let Some(ws_msg) = ws_receiver.next().await {
        match ws_msg {
            Ok(Message::Text(msg)) => {
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

    Msg::Nothing
}
