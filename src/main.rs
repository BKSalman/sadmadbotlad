use std::sync::Arc;

use sadmadbotlad::{eventsub, ApiInfo, flatten};
use eyre::WrapErr;
use irc::irc_connect;

mod util;
mod irc;
mod song_requests;

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

    let api_info = Arc::new(ApiInfo::new());
    
    let api_info_ref = api_info.clone();
    
    if let Err(e) = tokio::try_join!(
        flatten(tokio::spawn(async move {
            eventsub(api_info_ref).await
        })),
        flatten(tokio::spawn(async move {
            irc_connect(&api_info).await
        })),
    ).wrap_err_with(|| "run")
    {
        eprintln!("run:: {e}")
    };

    Ok(())
}
