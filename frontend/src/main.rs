use std::{cell::RefCell, rc::Rc};

use frontend::alerts::Alerts;
use frontend::songs::Songs;
use futures::{stream::SplitStream, StreamExt, SinkExt};
use gloo::console;
use gloo_net::websocket::{futures::WebSocket, Message};
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

#[derive(Clone, Routable, PartialEq)]
enum Route {
    #[at("/alerts")]
    Alerts,
    #[at("/songs")]
    Songs,
    #[not_found]
    #[at("/404")]
    NotFound,
}

pub struct App {
    alert: Option<String>,
    alert_msg: Option<String>,
    ws_receiver: Rc<RefCell<SplitStream<WebSocket>>>,
    sender: futures::channel::mpsc::Sender<String>,
    queue: String,
}

impl Component for App {
    type Message = frontend::Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let ws = WebSocket::open("ws://localhost:3000").expect("Ws");

        let (mut ws_sender, ws_receiver) = ws.split();

        let (sender, mut in_rx) = futures::channel::mpsc::channel::<String>(1000);

        spawn_local(async move {
            while let Some(msg) = in_rx.next().await {
                ws_sender.send(Message::Text(msg)).await.expect("send");
            }
        });

        let ws_receiver = Rc::new(RefCell::new(ws_receiver));
        ctx.link().send_future(do_stuff(ws_receiver.clone()));

        Self {
            alert: None,
            alert_msg: None,
            ws_receiver: ws_receiver.clone(),
            sender,
            queue: String::new(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            frontend::Msg::Follow(user) => {
                self.alert = Some(String::from("follow"));
                self.alert_msg = Some(format!("{user} followed 😎!"));
                console::log!(format!("{user}"));
                true
            }
            frontend::Msg::Raid(from) => {
                self.alert = Some(String::from("raid"));
                self.alert_msg = Some(format!("{from} raided 🦀!"));
                console::log!(format!("{from}"));
                true
            }
            frontend::Msg::RequestSongs => {
                {
                    let mut senderc = self.sender.clone();
                    spawn_local(async move {
                        senderc.send(String::from("songs")).await.expect("songs");
                    });
                }
                false
            }
            frontend::Msg::SongsResponse(queue) => {
                self.queue = queue;
                true
            }
            frontend::Msg::Clear(()) => {
                ctx.link().send_future(do_stuff(self.ws_receiver.clone()));
                self.alert = None;
                self.alert_msg = None;
                true
            }
            frontend::Msg::Nothing => {
                //TODO: make this better bitch
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let vid_cb = ctx.link().callback(|_| frontend::Msg::Clear(()));
        let songs_cb = ctx.link().callback(|_| frontend::Msg::RequestSongs);

        let queue = self.queue.clone();

        let alert = if let Some((alert, alert_msg)) = self.alert.clone().zip(self.alert_msg.clone()) {
                    html! { <Alerts {alert} {alert_msg} vid_cb={vid_cb.clone()} /> }
                } else {
                    html! { <Alerts vid_cb={vid_cb.clone()} /> }
                };
        let switch = move |routes: Route| match routes {
            Route::Alerts => alert.clone(),
            Route::Songs => html! { <Songs queue={queue.clone()} songs_cb={songs_cb.clone()} /> },
            Route::NotFound => html! { <h1>{ "404" }</h1> },
        };

        html! {
            <BrowserRouter>
                <Switch<Route> render={switch}/>
            </BrowserRouter>
        }
    }
}

async fn do_stuff(ws_receiver: Rc<RefCell<SplitStream<WebSocket>>>) -> frontend::Msg {
    if let Some(ws_msg) = ws_receiver.borrow_mut().next().await {
        match ws_msg {
            Ok(Message::Text(msg)) => {
                console::log!(format!("got {msg}"));
                let Some((event_type, arg)) = msg.split_once("::") else {
                    return frontend::Msg::Nothing;
                };
                match event_type {
                    "follow" => {
                        return frontend::Msg::Follow(arg.to_string());
                    }
                    "raid" => {
                        return frontend::Msg::Raid(arg.to_string());
                    }
                    "queue" => {
                        console::log!("song response");
                        return frontend::Msg::SongsResponse(arg.to_string());
                    }
                    _ => {
                        return frontend::Msg::Nothing;
                    }
                }
            }
            Ok(msg) => {
                console::log!(format!("{msg:?}"));
                return frontend::Msg::Nothing;
            }
            Err(e) => {
                console::log!(format!("{e:?}"));
                return frontend::Msg::Nothing;
            }
        }
    }

    frontend::Msg::Nothing
}

fn main() {
    yew::Renderer::<App>::new().render();
}
