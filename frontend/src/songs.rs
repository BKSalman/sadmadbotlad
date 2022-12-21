use std::{cell::RefCell, rc::Rc};

use futures::{stream::SplitStream, SinkExt, StreamExt};
use gloo::console;
use gloo_net::websocket::{futures::WebSocket, Message};
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::Queue;

pub enum Msg {
    RequestSongs,
    SongsResponse(String),
    Nothing,
}

pub struct Songs {
    sender: futures::channel::mpsc::Sender<Message>,
    queue: String,
}

impl Component for Songs {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        ctx.link().send_message(Msg::RequestSongs);
        let ws = WebSocket::open("wss://ws.bksalman.com").expect("Ws");

        let (mut ws_sender, ws_receiver) = ws.split();

        let (sender, mut in_rx) = futures::channel::mpsc::channel::<Message>(1000);

        spawn_local(async move {
            while let Some(msg) = in_rx.next().await {
                ws_sender.send(msg).await.expect("send");
            }
        });

        let ws_receiver = Rc::new(RefCell::new(ws_receiver));
        ctx.link().send_future(do_stuff(ws_receiver.clone()));

        Self {
            sender,
            queue: String::new(),
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::RequestSongs => {
                {
                    let mut senderc = self.sender.clone();
                    spawn_local(async move {
                        senderc
                            .send(Message::Text(String::from("songs")))
                            .await
                            .expect("songs");
                    });
                }
                false
            }
            Msg::SongsResponse(queue) => {
                self.queue = queue;
                true
            }
            Msg::Nothing => {
                //TODO: make this better bitch
                false
            }
        }
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        let queue: Queue = match serde_json::from_str(&self.queue) {
            Ok(queue) => queue,
            Err(_e) => Queue::default(),
        };

        let queue = if let Some(current_song) = queue.current_song {
            html! {
                <div class="songs-container">
                    {"Current song"}
                    <div class="song">
                        <div class ="title">
                            {
                                current_song.title
                            }
                        </div>
                        <a class ="url" target="_blank" href={current_song.url.clone()}>
                            {
                                current_song.url
                            }
                        </a>
                    </div>
                    {"Queue"}
                    {
                        queue.queue
                        .into_iter()
                        .filter(|s| s.is_some()).map(|s| {
                            html_nested! {
                                <div class="song">
                                    <div class ="title">
                                        {
                                            s.as_ref().unwrap().title.clone()
                                        }
                                    </div>
                                    <a class ="url" target="_blank" href={s.as_ref().unwrap().url.clone()} >
                                        {
                                            s.as_ref().unwrap().url.clone()
                                        }
                                    </a>
                                </div>
                            }
                        }).collect::<Html>()
                    }
                </div>
            }
        } else {
            html! {
                <div class="songs-container">{"Queue is empty"}</div>
            }
        };

        queue
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
                    "queue" => {
                        console::log!("song response");
                        return Msg::SongsResponse(arg.to_string());
                    }
                    _ => {
                        return Msg::Nothing;
                    }
                }
            }
            Ok(_msg) => {
                console::log!("something");
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
