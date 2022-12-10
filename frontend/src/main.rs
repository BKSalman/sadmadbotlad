use futures::{channel::mpsc::Sender, SinkExt, StreamExt};
use gloo::console;
use js_sys::Date;
use gloo_net::websocket::{futures::WebSocket, Message};
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

// Define the possible messages which can be sent to the component
pub enum Msg {
    Increment,
    Decrement,
    Send,
}

pub struct App {
    value: i64, // This will store the counter value
    sender: Sender<String>,
}

impl Component for App {
    type Message = Msg;
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        let ws = WebSocket::open("ws://localhost:3000").expect("Ws");

        let (mut ws_sender, mut receiver) = ws.split();

        let (sender, mut in_rx) = futures::channel::mpsc::channel::<String>(1000);

        spawn_local(async move {
            while let Some(msg) = in_rx.next().await {
                ws_sender.send(Message::Text(msg)).await.expect("send");
            }
        });
        
        spawn_local(async move {
            while let Some(ws_msg) = receiver.next().await {
                match ws_msg {
                    Ok(msg) => {
                        console::log!(format!("{msg:?}"));
                    }
                    Err(e) => console::log!(format!("{e:?}")),
                }
            }
        });
        Self { value: 0, sender }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Increment => {
                self.value += 1;
                console::log!("plus one"); // Will output a string to the browser console
                true // Return true to cause the displayed change to update
            }
            Msg::Decrement => {
                self.value -= 1;
                console::log!("minus one");
                true
            }
            Msg::Send => {
                self.sender
                    .try_send(String::from("Something"))
                    .expect("channel sender");
                console::log!("sent");
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div>
                <div class="panel">
                    <button onclick={ctx.link().callback(|_| Msg::Send )}>
                        { "lmao" }
                    </button>

                </div>

                // Display the current value of the counter
                <p class="counter">
                    { self.value }
                </p>

                // Display the current date and time the page was rendered
                <p class="footer">
                    { "Rendered: " }
                    { String::from(Date::new_0().to_string()) }
                </p>
            </div>
        }
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}
