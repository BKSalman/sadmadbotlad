use frontend::activity_feed::Activity;
use frontend::alerts::Alerts;
use frontend::code::Code;
use frontend::songs::Songs;
use frontend::Route;
use yew::prelude::*;
use yew_router::prelude::*;

pub struct App {}

impl Component for App {
    type Message = ();
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        Self {}
    }

    fn update(&mut self, _ctx: &Context<Self>, _msg: Self::Message) -> bool {
        false
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        let switch = move |routes: Route| match routes {
            Route::Alerts => html! { <Alerts /> },
            Route::Songs => html! { <Songs /> },
            Route::Activity => html! { <Activity /> },
            Route::Code => html! { <Code /> },
            Route::NotFound => html! { <h1>{ "404" }</h1> },
        };

        html! {
            <BrowserRouter>
                <Switch<Route> render={switch}/>
            </BrowserRouter>
        }
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}
