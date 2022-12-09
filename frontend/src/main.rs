use gloo::console;
use js_sys::Date;
use futures_util::{SinkExt, StreamExt};
use reqwasm::websocket::{futures::WebSocket, Message};
use yew::{prelude::*, platform::spawn_local};


#[function_component]
fn App() -> Html {
    let counter = use_state(|| 0);
    let onclick = {
        let counter = counter.clone();
        move |_| {
            let value = *counter + 1;
            counter.set(value);
        }
    };

    html! {
        <div>
            <button {onclick}>{ "+1" }</button>
            <p>{ *counter }</p>
        </div>
    }
}


// #[tokio::main]
fn main() -> Result<(), eyre::Report> {
    let ws = WebSocket::open("ws://localhost:3000")?;

    let (mut sender, mut receiver) = ws.split();

    spawn_local(async move {
        if let Err(e) = sender.send(Message::Text(String::from("Hello :)"))).await {
            println!("{e}");
        }
    });
    
    spawn_local(async move {
        while let Some(s) = receiver.next().await {
            println!("{s:?}");
            
        }
    });

    yew::Renderer::<App>::new().render();

    Ok(())
}
