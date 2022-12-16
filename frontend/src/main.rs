use std::{cell::RefCell, rc::Rc, time::Duration};

use futures::{channel::mpsc::Sender, stream::SplitStream, FutureExt, SinkExt, StreamExt};
use gloo::console;
use gloo_net::websocket::{futures::WebSocket, Message};
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

pub enum Msg {
    Follow(String),
    Raid(String),
    Clear(()),
    Nothing,
}

pub struct App {
    alert: Option<String>,
    alert_msg: Option<String>,
    // alert_cb: Callback<Msg>,
    ws_receiver: Rc<RefCell<SplitStream<WebSocket>>>,
}

impl Component for App {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let ws = WebSocket::open("ws://localhost:3000").expect("Ws");

        let (_ws_sender, ws_receiver) = ws.split();

        // let (sender, mut in_rx) = futures::channel::mpsc::channel::<String>(1000);

        // spawn_local(async move {
        //     while let Some(msg) = in_rx.next().await {
        //         ws_sender.send(Message::Text(msg)).await.expect("send");
        //     }
        // });

        // let cb = ctx.link().callback(|message: Msg| message);

        let ws_receiver = Rc::new(RefCell::new(ws_receiver));
        ctx.link().send_future(do_stuff(ws_receiver.clone()));

        // let cbc = cb.clone();

        // spawn_local(async move {
        //     while let Some(ws_msg) = ws_receiver.next().await {
        //         match ws_msg {
        //             Ok(Message::Text(msg)) => {
        //                 console::log!(format!("got {msg}"));
        //                 let Some((event_type, user)) = msg.split_once("::") else {
        //                     continue;
        //                 };
        //                 match event_type {
        //                     "follow" => {
        //                         cbc.emit(Msg::Follow(user.to_string()));
        //                         // yew::platform::time::sleep(Duration::from_secs(4)).await;
        //                         // cbc.emit(Msg::Clear);
        //                     }
        //                     _ => {}
        //                 }
        //             }
        //             Ok(msg) => {
        //                 console::log!(format!("{msg:?}"));
        //             }
        //             Err(e) => console::log!(format!("{e:?}")),
        //         }
        //     }
        // });

        Self {
            alert: None,
            alert_msg: None,
            // alert_cb: cb,
            ws_receiver: ws_receiver.clone(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Follow(user) => {
                self.alert = Some(String::from("follow"));
                self.alert_msg = Some(format!("{user} followed 😎!"));
                console::log!(format!("{user}"));
                true
            }
            Msg::Raid(from) => {
                self.alert = Some(String::from("raid"));
                self.alert_msg = Some(format!("{from} raided 🦀!"));
                console::log!(format!("{from}"));
                true
            }
            Msg::Clear(()) => {
                ctx.link().send_future(do_stuff(self.ws_receiver.clone()));
                self.alert = None;
                self.alert_msg = None;
                true
            }
            Msg::Nothing => {
                console::log!("make this better bitch");
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        if let Some(alert) = &self.alert {
            let src = format!("assets/{}.webm", alert);
            html! {
                <div class="flex">
                    <div class="vid">
                        <video src={src} onended={ctx.link().callback(|_| Msg::Clear(()))} autoplay=true/>
                    </div>
                    <p class="text">{ self.alert_msg.clone() }</p>
                </div>
            }
        } else {
            html! {
                    <></>
            }
        }
    }
}

async fn do_stuff(ws_receiver: Rc<RefCell<SplitStream<WebSocket>>>) -> Msg {
    if let Some(ws_msg) = ws_receiver.borrow_mut().next().await {
        match ws_msg {
            Ok(Message::Text(msg)) => {
                console::log!(format!("got {msg}"));
                let Some((event_type, user)) = msg.split_once("::") else {
                    return Msg::Nothing;
                };
                match event_type {
                    "follow" => {
                        return Msg::Follow(user.to_string());
                    }
                    "raid" => {
                        return Msg::Raid(user.to_string());
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

fn main() {
    yew::Renderer::<App>::new().render();
}
