use yew::prelude::*;

pub enum Msg {
    Clear(()),
}

pub struct Alerts {
}

#[derive(Properties, PartialEq)]
pub struct Props {
    #[prop_or_default]
    pub alert: Option<String>,
    #[prop_or_default]
    pub alert_msg: Option<String>,
    pub vid_cb: Callback<super::Msg>
}

impl Component for Alerts {
    type Message = Msg;
    type Properties = Props;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {}
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Clear(()) => {
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let vid_cb = {
            let original_cb = ctx.props().vid_cb.clone();
            move |_event: Event| {
                original_cb.emit(super::Msg::Clear(()))
            }
        };

        if let Some(alert) = &ctx.props().alert {
            let src = format!("assets/{}.webm", alert);
            html! {
                <div class="flex">
                    <div class="vid">
                        <video src={src} onended={vid_cb} autoplay=true/>
                    </div>
                    <p class="text">{ ctx.props().alert_msg.clone() }</p>
                </div>
            }
        } else {
            html! {
                    <></>
            }
        }
    }
}

