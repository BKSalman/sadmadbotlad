use std::path::Path;
use tokio::net::TcpListener;
use topcoat::{
    asset::{AssetBundle, RouterBuilderAssetExt},
    router::{Methods, Router, RouterBuilderDiscoverExt, tower::TowerRoute},
    view::view,
};
use tower_http::services::ServeDir;

use crate::{Alert, db::DBMessage};

mod pages;

struct AppContext {
    alerts_sender: tokio::sync::broadcast::Sender<Alert>,
    code_sender: tokio::sync::mpsc::Sender<String>,
    db_tx: std::sync::mpsc::Sender<DBMessage>,
}

pub async fn run_server(
    port: u16,
    static_path: impl AsRef<Path>,
    alerts_sender: tokio::sync::broadcast::Sender<Alert>,
    db_tx: std::sync::mpsc::Sender<DBMessage>,
    code_sender: tokio::sync::mpsc::Sender<String>,
) -> anyhow::Result<()> {
    const HOST_ENV: &str = "HOST";
    const DEFAULT_HOST: &str = "127.0.0.1";

    let host = match std::env::var(HOST_ENV) {
        Ok(value) => Ok(value),
        Err(std::env::VarError::NotPresent) => Ok(DEFAULT_HOST.to_owned()),
        Err(error) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{HOST_ENV} must be valid Unicode: {error}"),
        )),
    }?;

    tracing::info!("running web server on {}:{}", host, port);
    tracing::info!("static path is: {}", static_path.as_ref().display());

    let listener = TcpListener::bind((host.as_str(), port)).await?;

    let router = Router::builder()
        .discover()
        .assets(AssetBundle::load()?)
        .route(TowerRoute::new(
            Methods::Any,
            "/assets/{*path}",
            ServeDir::new(static_path),
        ))
        .app_context(AppContext {
            alerts_sender,
            code_sender,
            db_tx,
        })
        .build();

    topcoat::serve(listener, router).await?;

    Ok(())
}
