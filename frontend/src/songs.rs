use futures::StreamExt;
use gloo::console;
use gloo_net::websocket::{futures::WebSocket, Message};
use yew::prelude::*;

use crate::Queue;

pub enum Msg {
    SongsResponse(String),
    Nothing,
}

pub struct Songs {
    queue: String,
}

impl Component for Songs {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let ws = WebSocket::open("wss://ws.bksalman.com").expect("Ws");

        ctx.link().send_future(do_stuff(ws));

        Self {
            queue: String::new(),
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
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

async fn do_stuff(mut ws: WebSocket) -> Msg {
    let mut yew_msg = Msg::Nothing;

    while let Some(ws_msg) = ws.next().await {
        match ws_msg {
            Ok(Message::Text(msg)) => {
                console::log!("got {}", &msg);
                yew_msg = Msg::SongsResponse(msg);
                break;
            }
            Err(e) => {
                console::log!(format!("{e:?}"));
                break;
            }
            _ => {}
        }
    }

    ws.close(Some(1000), None).expect("close ws");
    yew_msg
}
