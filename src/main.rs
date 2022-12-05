use sadmadbotlad::flatten;
use eyre::WrapErr;

use sadmadbotlad::{
    irc::irc_connect,
    eventsub::eventsub,
};

mod util;
use util::install_eyre;

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    install_eyre()?;
    
    run().await.with_context(|| "main:: running application")?;
    
    Ok(())
}

async fn run() -> Result<(), eyre::Report> {

    // let live = LiveStatus::Offline {
    //     url: String::from("https://twitch.tv/sadmadladsalman"),
    // };

    // let (sender, recv) = watch::channel(live);

    tokio::try_join!(
        flatten(tokio::spawn(async move {
            eventsub().await
        })),
        flatten(tokio::spawn(async move {
            irc_connect().await
        })),
    ).wrap_err_with(|| "run")?;

    Ok(())
}
