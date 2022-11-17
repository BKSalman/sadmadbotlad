use eyre::WrapErr;
use reqwest::Url;
use tokio_tungstenite::connect_async;

use sadmadbotlad::{ApiInfo, discord::send_notification};

mod util;
use util::install_eyre;

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    install_eyre()?;

    run().await.with_context(|| "when running application")?;

    Ok(())
}

async fn run() -> Result<(), eyre::Report> {
    let api_info = ApiInfo::new();

    let (socket, _response) =
        connect_async(Url::parse("wss://eventsub-beta.wss.twitch.tv/ws").expect("Url parsed"))
            .await?;

    // let live = LiveStatus::Offline {
    //     url: String::from("https://twitch.tv/sadmadladsalman"),
    // };

    // let (sender, recv) = watch::channel(live);

    api_info.handle_socket(socket).await;

    Ok(())
}
