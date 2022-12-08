use eyre::WrapErr;
use tokio_retry::{strategy::ExponentialBackoff, Retry};

use sadmadbotlad::flatten;

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
    tokio::try_join!(
        flatten(tokio::spawn(eventsub())),
        flatten(tokio::spawn(irc_connect())),
    )
    .wrap_err_with(|| "Run")?;

    Ok(())
}
