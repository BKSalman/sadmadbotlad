use sadmadbotlad::{eventsub, ApiInfo};
use eyre::WrapErr;
use irc::irc_connect;

// use sadmadbotlad::eventsub;

mod util;
mod irc;
mod song_requests;

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
    let api_info = ApiInfo::new();
    
    irc_connect(&api_info).await?;

    eventsub(api_info).await?;

    Ok(())
}
