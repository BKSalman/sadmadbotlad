use frontend::alerts::Alerts;
use frontend::songs::Songs;
use yew::prelude::*;
use yew_router::prelude::*;

#[derive(Clone, Routable, PartialEq)]
enum Route {
    #[at("/alerts")]
    Alerts,
    #[at("/")]
    Songs,
    #[not_found]
    #[at("/404")]
    NotFound,
}

pub struct App {}

impl Component for App {
    type Message = frontend::Msg;
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
