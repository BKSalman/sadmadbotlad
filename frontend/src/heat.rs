use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use futures_util::{SinkExt, StreamExt};
use gloo::console::{self, __macro::JsValue};
use gloo::timers::callback::Interval;
use gloo_net::http::Request;
use gloo_net::websocket::{futures::*, Message};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

pub struct Heat {
    users: Arc<RwLock<HashMap<String, User>>>,
}

pub enum Msg {
    GetDetails(f32, f32, String),
    ShowPfp(User),
    Nothing,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct HeatEvent {
    r#type: String,
    #[serde(flatten)]
    click: Click,
    #[serde(flatten)]
    system: System,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Click {
    id: Option<String>,
    x: Option<String>,
    y: Option<String>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct System {
    message: Option<String>,
    channel: Option<i32>,
    version: Option<String>,
}

impl From<HeatEvent> for JsValue {
    fn from(value: HeatEvent) -> Self {
        JsValue::from_str(&format!("{value:#?}"))
    }
}

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
pub struct User {
    id: String,
    display_name: String,
    profile_img: String,
    position: (f32, f32),
}

impl Component for Heat {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        //sadmadladsalman: 143306668
        //soulsev: 50475354
        let (sender, mut receiver) = WebSocket::open("wss://heat-api.j38.net/channel/143306668")
            .expect("ws")
            .split();

        let sender = Arc::new(RwLock::new(sender));

        Interval::new(5000, move || {
            let sender = sender.clone();
            spawn_local(async move {
                console::log!("ping");
                sender
                    .write()
                    .expect("sender")
                    .send(Message::Bytes(vec![]))
                    .await
                    .expect("Ping");
            });
        })
        .forget();

        let link = ctx.link().clone();
        spawn_local(async move {
            while let Some(msg) = receiver.next().await {
                match msg {
                    Ok(Message::Text(msg)) => {
                        let event = serde_json::from_str::<HeatEvent>(&msg).expect("Heat event");
                        match event.r#type.as_str() {
                            "system" => {
                                console::log!("Connected");
                            }
                            "click" => {
                                let id = event.click.id.expect("user id");
                                let x = event.click.x.expect("x");
                                let y = event.click.y.expect("y");

                                match &id[0..1] {
                                    "U" => {
                                        console::log!("Unauthorized user");
                                    }
                                    "A" => {
                                        console::log!("Anonymous user");
                                    }
                                    _ => {
                                        let x = x.parse::<f32>().expect("x");
                                        let y = y.parse::<f32>().expect("x");
                                        link.send_message(Msg::GetDetails(x, y, id));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(msg) => {
                        console::log!(format!("{:#?}", msg));
                    }
                    Err(e) => console::log!(format!("{:#?}", e)),
                }
            }
        });

        Self {
            users: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::ShowPfp(user) => {
                console::log!(format!("{:#?}", user));
                if let None = self
                    .users
                    .write()
                    .expect("write user")
                    .insert(user.id.clone(), user)
                {
                    console::log!("new user");
                }
                true
            }
            Msg::GetDetails(x, y, id) => {
                ctx.link()
                    .send_future(get_user_details(self.users.clone(), x, y, id));
                true
            }
            Msg::Nothing => todo!(),
        }
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        html! {
            <>
                {
                    self.users.read().expect("read users").iter().map(|(_, user)| {
                        let styles = format!(
                                "
                                    position: absolute;
                                    top: calc({}% - 30px);
                                    left: calc({}% - 30px);
                                ",
                                user.position.1, user.position.0
                            );
                        html_nested! {
                            <img style={styles} src={user.profile_img.clone()} width="60px" height="60px"/>
                        }
                    }).collect::<Html>()
                }
            </>
        }
    }
}

type Users = Arc<RwLock<HashMap<String, User>>>;

async fn get_user_details(users: Users, x: f32, y: f32, id: String) -> Msg {
    if let Some(user) = users.read().expect("read users").get(&id) {
        console::log!("Cached");
        console::log!(&user.display_name);
        return Msg::ShowPfp(User {
            id,
            display_name: user.display_name.clone(),
            profile_img: user.profile_img.clone(),
            position: (x * 100., y * 100.),
        });
    }

    console::log!("Requesting user info");

    let res = Request::get(&format!("https://heat-api.j38.net/user/{}", id))
        .send()
        .await;

    if let Ok(res) = res {
        let res = res.json::<Value>().await.expect("json");

        let display_name = res["display_name"].as_str().expect("str").to_string();
        let profile_img = res["profile_image_url"].as_str().expect("str").to_string();

        return Msg::ShowPfp(User {
            id,
            display_name,
            profile_img,
            position: (x * 100., y * 100.),
        });
    }

    Msg::Nothing
}
