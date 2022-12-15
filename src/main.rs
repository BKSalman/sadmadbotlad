use eyre::WrapErr;
use sadmadbotlad::FrontEndEvent;
use tokio_retry::{strategy::ExponentialBackoff, Retry};

use sadmadbotlad::{flatten, ws_server::ws_server};

use sadmadbotlad::{eventsub::eventsub, irc::irc_connect};

mod util;
use util::install_eyre;

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    install_eyre()?;

    Retry::spawn(ExponentialBackoff::from_millis(100).take(5), || run())
        .await
        .with_context(|| "main:: running application")?;

    Ok(())
}

async fn run() -> Result<(), eyre::Report> {
    let (sender, _) = tokio::sync::broadcast::channel::<FrontEndEvent>(100);
    
    tokio::try_join!(
        flatten(tokio::spawn(eventsub(sender.clone()))),
        flatten(tokio::spawn(irc_connect())),
        flatten(tokio::spawn(ws_server(sender.clone()))),
    )
    .wrap_err_with(|| "Run")?;

    Ok(())
}
