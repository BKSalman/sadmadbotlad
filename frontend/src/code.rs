use futures::SinkExt;
use gloo_net::websocket::{futures::WebSocket, Message};
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::Route;

#[derive(Deserialize, Serialize)]
struct Queries {
    code: String,
    scope: String,
}

#[function_component]
pub fn Code() -> Html {
    let location = use_location().unwrap();
    let navigator = use_navigator().unwrap();

    let mut ws = WebSocket::open("ws://localhost:4040").expect("ws");

    let code = location.query::<Queries>().unwrap().code;

    spawn_local(async move {
        ws.send(Message::Text(code)).await.expect("send to ws");

        ws.close(Some(1000), None).expect("close ws connection");

        navigator.push(&Route::Activity);
    });

    html! {
        <></>
    }
}
