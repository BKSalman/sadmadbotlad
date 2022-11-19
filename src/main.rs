use eyre::WrapErr;
use irc::{irc_connect, SongRequest, Queue};

// use sadmadbotlad::eventsub;

mod util;
mod irc;

use util::install_eyre;

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    install_eyre()?;

    run().await.with_context(|| "when running application")?;

    Ok(())
}

async fn run() -> Result<(), eyre::Report> {

    // let live = LiveStatus::Offline {
    //     url: String::from("https://twitch.tv/sadmadladsalman"),
    // };

    // let (sender, recv) = watch::channel(live);
    
    irc_connect().await?;

    // eventsub().await?;

    Ok(())
}
