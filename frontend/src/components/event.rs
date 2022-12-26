use yew::{function_component, html, Html, Properties, Callback, MouseEvent, AttrValue};

use crate::components::replay::ReplayBtn;

#[derive(Properties, PartialEq)]
pub struct Props {
    pub text: AttrValue,
    pub on_click: Callback<MouseEvent>,
}

#[function_component]
pub fn Event(props: &Props) -> Html {
    html! {
        <div class="event">
            <div class="event-args">
                { props.text.clone() }
            </div>
            < ReplayBtn on_click={props.on_click.clone()}/>
        </div>
    }
}
