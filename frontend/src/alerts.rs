use std::{cell::RefCell, rc::Rc};

use futures::{stream::SplitStream, StreamExt};
use gloo::console;
use gloo_net::websocket::{futures::WebSocket, Message};
use yew::prelude::*;

pub enum Msg {
    Follow(String),
    Raid(String),
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
        let ws = WebSocket::open("wss://ws.bksalman.com").expect("Ws");

        let (_, ws_receiver) = ws.split();

        let ws_receiver = Rc::new(RefCell::new(ws_receiver));
        ctx.link().send_future(do_stuff(ws_receiver.clone()));

        Self {
            alert: None,
            alert_msg: None,
            ws_receiver: ws_receiver.clone(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Follow(user) => {
                self.alert = Some(String::from("follow"));
                self.alert_msg = Some(format!("{user} followed 😎!"));
                true
            }
            Msg::Raid(from) => {
                self.alert = Some(String::from("raid"));
                self.alert_msg = Some(format!("{from} raided 🦀!"));
                true
            }
            Msg::Clear(()) => {
                ctx.link().send_future(do_stuff(self.ws_receiver.clone()));
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
        let vid_cb = {
            let ctxc = ctx.link().clone();
            move |_event: Event| ctxc.send_message(Msg::Clear(()))
        };

        let Some(alert) = &self.alert else {
            return html! {
                    <></>
            };
        };

        let src = format!("assets/{}.webm", alert);
        html! {
            <div class="flex">
                <div class="vid">
                    <video src={src} onended={vid_cb} autoplay=true/>
                </div>
                <p class="text">{ self.alert_msg.clone() }</p>
            </div>
        }
    }
}

async fn do_stuff(ws_receiver: Rc<RefCell<SplitStream<WebSocket>>>) -> Msg {
    if let Some(ws_msg) = ws_receiver.borrow_mut().next().await {
        match ws_msg {
            Ok(Message::Text(msg)) => {
                console::log!("got {}", &msg);
                let Some((event_type, arg)) = msg.split_once("::") else {
                    return Msg::Nothing;
                };
                match event_type {
                    "follow" => {
                        return Msg::Follow(arg.to_string());
                    }
                    "raid" => {
                        return Msg::Raid(arg.to_string());
                    }
                    _ => {
                        return Msg::Nothing;
                    }
                }
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
