use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
};

use axum::{http::StatusCode, routing::get_service, Router, Server};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

#[tokio::main]
async fn main() {
    let mut args = std::env::args();

    let Some(static_path) = args.nth(1) else {
        eprintln!("no path was specified");
        std::process::exit(1);
    };

    let Some(port) = args.nth(0) else {
        eprintln!("no port provided");
        std::process::exit(1);
    };

    let router = Router::new().fallback_service(
        Router::new().nest_service(
            "/",
            get_service(ServeDir::new(&static_path).fallback(ServeFile::new(
                PathBuf::from(static_path).join("index.html"),
            )))
            .handle_error(|error| async move {
                tracing::error!(?error, "failed serving static file");
                StatusCode::INTERNAL_SERVER_ERROR
            })
            .layer(TraceLayer::new_for_http()),
        ),
    );

    let addr = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        port.parse::<u16>().expect("port should be valid"),
    );

    Server::bind(&addr)
        .serve(router.into_make_service())
        .await
        .expect("start server");
}
