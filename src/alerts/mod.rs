use yew::{html, Component, Context, Html};

enum Message {
    
}

pub struct App;

impl Component for App {
    type Message = Message;

    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        Self
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        html! {
            <div>
                { "lmao" }
            </div>
        }
    }
}