use yew::prelude::*;

use crate::Queue;

pub enum Msg {}

pub struct Songs {}

#[derive(Properties, PartialEq)]
pub struct Props {
    pub songs_cb: Callback<super::Msg>,
    pub queue: String,
}

impl Component for Songs {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        ctx.props().songs_cb.emit(crate::Msg::RequestSongs);

        Self {}
    }

    fn update(&mut self, _ctx: &Context<Self>, _msg: Self::Message) -> bool {
        false
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let queue: Queue = match serde_json::from_str(&ctx.props().queue) {
            Ok(queue) => queue,
            Err(_e) => Queue::default(),
        };

        let queue = if let Some(current_song) = queue.current_song {
            html! {
                <div class="songs-container">
                    {"Current song"}
                    <div class="song">
                        <div class ="title">
                            {
                                current_song.title
                            }
                        </div>
                        <a href={current_song.url.clone()} class ="url">
                            {
                                current_song.url
                            }
                        </a>
                    </div>
                    {"Queue"}
                    {
                        queue.queue
                        .into_iter()
                        .filter(|s| s.is_some()).map(|s| {
                            html_nested! {
                                <div class="song">
                                    <div class ="title">
                                        {
                                            s.as_ref().unwrap().title.clone()
                                        }
                                    </div>
                                    <a href={s.as_ref().unwrap().url.clone()} class ="url">
                                        {
                                            s.as_ref().unwrap().url.clone()
                                        }
                                    </a>
                                </div>
                            }
                        }).collect::<Html>()
                    }
                </div>
            }
        } else {
            html! {
                <div class="songs-container">{"Queue is empty"}</div>
            }
        };

        queue
    }
}
