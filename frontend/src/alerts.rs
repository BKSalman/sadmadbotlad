use std::{cell::RefCell, rc::Rc};

use futures::{stream::SplitStream, StreamExt};
use gloo::console;
use gloo_net::websocket::{futures::WebSocket, Message};
use yew::prelude::*;

use crate::{components::alert::Alert, Alert as AlertEnum, AlertEventType};

pub enum Msg {
    Event(AlertEnum),
    Clear(()),
    Nothing,
}

pub struct Alerts {
    alert: Option<String>,
    alert_msg: Option<String>,
    ws_receiver: Rc<RefCell<SplitStream<WebSocket>>>,
}

impl Component for Alerts {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let ws = WebSocket::open("ws://localhost:4000").expect("Ws");

        let (_, ws_receiver) = ws.split();

        let ws_receiver = Rc::new(RefCell::new(ws_receiver));
        ctx.link().send_future(handle_alert(ws_receiver.clone()));

        Self {
            alert: None,
            alert_msg: None,
            ws_receiver: ws_receiver.clone(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Event(alert) => {
                match alert.alert_type {
                    AlertEventType::Follow { follower } => {
                        self.alert = Some(String::from("follow"));
                        self.alert_msg = Some(format!("{follower} followed 😎!"));
                    }
                    AlertEventType::Raid { from, viewers } => {
                        self.alert = Some(String::from("raid"));
                        self.alert_msg = Some(format!("{from} raided with {viewers} viewers 🦀!"));
                    }
                    AlertEventType::Subscribe { subscriber, tier } => {
                        self.alert = Some(String::from("sub"));
                        self.alert_msg = Some(format!("{subscriber} subscribed with tier {tier}!"));
                    }
                    AlertEventType::GiftSub {
                        gifter,
                        total,
                        tier,
                    } => {
                        self.alert = Some(String::from("sub"));
                        self.alert_msg = Some(format!("{gifter} gifted {total} tier {tier} subs!"));
                    }
                    AlertEventType::ReSubscribe {
                        subscriber,
                        tier,
                        subscribed_for: _,
                        streak,
                    } => {
                        self.alert = Some(String::from("sub"));
                        if streak > 1 {
                            self.alert_msg = Some(format!(
                                "{subscriber} resubscribed with a streak of {streak} months!!!"
                            ));
                            return true;
                        }
                        self.alert_msg =
                            Some(format!("{subscriber} resubscribed with tier {tier}!"));
                    }
                    AlertEventType::Gifted { gifted, tier } => {
                        self.alert = Some(String::from("sub"));
                        self.alert_msg = Some(format!("{gifted} got gifted a tier {tier} sub!"));
                    }
                }
                true
            }
            Msg::Clear(()) => {
                ctx.link()
                    .send_future(handle_alert(self.ws_receiver.clone()));
                self.alert = None;
                self.alert_msg = None;
                true
            }
            Msg::Nothing => {
                //TODO: make this better bitch
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let onended = Callback::from({
            let ctxc = ctx.link().clone();
            move |_event: Event| ctxc.send_message(Msg::Clear(()))
        });

        let Some(alert) = &self.alert else {
            return html! {
                    <></>
            };
        };

        let src = format!("assets/{}.webm", alert);
        html! {
            < Alert src={src} onended={onended} alert_msg={self.alert_msg.clone()}/>
        }
    }
}

async fn handle_alert(ws_receiver: Rc<RefCell<SplitStream<WebSocket>>>) -> Msg {
    if let Some(ws_msg) = ws_receiver.borrow_mut().next().await {
        match ws_msg {
            Ok(Message::Text(msg)) => {
                let event = serde_json::from_str::<AlertEnum>(&msg).expect("event");
                console::log!("got {}", &msg);
                return Msg::Event(event);
            }
            Ok(msg) => {
                console::log!(format!("{msg:?}"));
                return Msg::Nothing;
            }
            Err(e) => {
                console::log!(format!("{e:?}"));
                return Msg::Nothing;
            }
        }
    }

    Msg::Nothing
}
