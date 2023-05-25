use hebi::{Hebi, NativeModule, Scope, Str, This};
use tokio::sync::{
    broadcast,
    mpsc::{self, Sender},
};
use tokio_tungstenite::tungstenite::Message;

use crate::{irc::to_irc_message, twitch::TwitchTokenMessages, Alert};

struct WsSender(Sender<Message>);

impl WsSender {
    async fn send(scope: Scope<'_>, this: This<'_, Self>) -> hebi::Result<()> {
        let original_message = scope.param::<Str>(0)?;
        let mut message = String::new();
        if scope.num_args() > 1 {
            if let Some(extra) = scope.param::<Option<Str>>(1)? {
                message = format!("{original_message}{extra}");
            }
        } else {
            message = original_message.as_str().to_string();
        }

        this.0
            .send(Message::Text(to_irc_message(message)))
            .await
            .map_err(hebi::Error::user)?;

        Ok(())
    }
}

struct AlertSender(broadcast::Sender<Alert>);

impl AlertSender {
    async fn send(scope: Scope<'_>, this: This<'_, Self>) -> hebi::Result<()> {
        let args = scope.param::<Str>(0)?;

        let alert = match args.to_string().to_lowercase().as_str() {
            "raid" => Alert::raid_test(),
            "follow" => Alert::follow_test(),
            "sub" => Alert::sub_test(),
            "resub" => Alert::resub_test(),
            "giftsub" => Alert::giftsub_test(),
            "asd" => Alert::asd_test(),
            _ => Alert::raid_test(),
        };

        this.0.send(alert).map_err(hebi::Error::user)?;

        Ok(())
    }
}

struct TwitchClient(mpsc::UnboundedSender<TwitchTokenMessages>);

impl TwitchClient {
    async fn get_title(_scope: Scope<'_>, this: This<'_, Self>) -> hebi::Result<String> {
        let title = crate::twitch::get_title(this.0.clone())
            .await
            .map_err(hebi::Error::user)?;

        Ok(title)
    }

    async fn set_title(scope: Scope<'_>, this: This<'_, Self>) -> hebi::Result<()> {
        let title = scope.param::<Str>(0)?;

        crate::twitch::set_title(title.as_str(), this.0.clone())
            .await
            .map_err(hebi::Error::user)?;

        Ok(())
    }
}

pub async fn run_hebi(
    irc_sender: Sender<Message>,
    alert_sender: broadcast::Sender<Alert>,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
) -> Result<Hebi, hebi::Error> {
    let mut vm = Hebi::new();

    let module = NativeModule::builder("ws")
        .class::<WsSender>("WsSender", |class| {
            class.async_method("send", WsSender::send).finish()
        })
        .class::<AlertSender>("AlertSender", |class| {
            class.async_method("send", AlertSender::send).finish()
        })
        .class::<TwitchClient>("TwitchClient", |class| {
            class
                .async_method("get_title", TwitchClient::get_title)
                .async_method("set_title", TwitchClient::set_title)
                .finish()
        })
        .finish();

    vm.register(&module);

    vm.global().set(
        vm.new_string("ws_sender"),
        vm.new_instance(WsSender(irc_sender))?,
    );

    vm.global().set(
        vm.new_string("alert_sender"),
        vm.new_instance(AlertSender(alert_sender))?,
    );

    vm.global().set(
        vm.new_string("twitch_client"),
        vm.new_instance(TwitchClient(token_sender))?,
    );

    vm.eval_async(
        r#"
ws_sender.send("YEP")
        "#,
    )
    .await?;

    Ok(vm)
}
