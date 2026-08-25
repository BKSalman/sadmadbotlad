use topcoat::{
    context::{Cx, app_context},
    router::{error::not_found, page, request::uri},
    view::view,
};

use crate::web::AppContext;

#[page("/code")]
async fn code(cx: &Cx) -> topcoat::Result {
    let mut code = None;
    let mut scope = None;

    let Some(queries) = uri(cx).query() else {
        return Err(not_found().into());
    };
    let mut queries = queries.split("&");
    let first_query = queries.next().and_then(|q| q.split_once('='));
    let second_query = queries.next().and_then(|q| q.split_once('='));
    if let Some((key, value)) = first_query {
        if key == "code" {
            code = Some(value.to_string());
        } else if key == "scope" {
            scope = Some(value.to_string());
        }
    }
    if let Some((key, value)) = second_query {
        if key == "code" {
            code = Some(value.to_string());
        } else if key == "scope" {
            scope = Some(value.to_string());
        }
    }

    if let Some(code) = &code {
        let code_sender = app_context::<AppContext>(cx).code_sender.clone();

        code_sender.send(code.clone()).await?;
    }

    view! {
        <p>(code)</p>
        <p>(scope)</p>
    }
}

// let location = use_location().unwrap();
// let navigator = use_navigator().unwrap();

// let mut ws = WebSocket::open("ws://localhost:4040").expect("ws");

// let code = location.query::<Queries>().unwrap().code;

// spawn_local(async move {
//     ws.send(Message::Text(code)).await.expect("send to ws");

//     ws.close(Some(1000), None).expect("close ws connection");

//     navigator.push(&Route::Activity);
// });

// html! {
//     <></>
// }
