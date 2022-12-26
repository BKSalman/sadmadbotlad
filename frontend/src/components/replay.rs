use yew::{function_component, html, Html, Properties, Callback, MouseEvent};
use yew_icons::{Icon, IconId};

#[derive(Properties, PartialEq)]
pub struct Props {
    pub on_click: Callback<MouseEvent>,
}

#[function_component]
pub fn ReplayBtn(props: &Props) -> Html {
    html! { <Icon class="replay-btn" onclick={props.on_click.clone()} icon_id={IconId::FontAwesomeSolidReply}/> }
}
