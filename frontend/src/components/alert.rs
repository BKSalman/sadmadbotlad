use yew::{function_component, html, Html, Properties, Callback, AttrValue};

#[derive(Properties, PartialEq)]
pub struct Props {
    pub src: AttrValue,
    pub onended: Callback<yew::Event>,
    pub alert_msg: Option<AttrValue>,
}

#[function_component]
pub fn Alert(props: &Props) -> Html {
    html! {
        <div class="flex">
            <div class="vid">
                <video src={props.src.clone()} onended={props.onended.clone()} autoplay=true/>
            </div>
            <p class="text">{ props.alert_msg.clone() }</p>
        </div>
    }
}
