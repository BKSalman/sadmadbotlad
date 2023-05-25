use hebi::{Hebi, NativeModule, Scope, Str, This};
use tokio::sync::{broadcast, mpsc::Sender};
use tokio_tungstenite::tungstenite::Message;

use crate::{irc::to_irc_message, Alert};

struct WsSender(Sender<Message>);

impl WsSender {
    async fn send(scope: Scope<'_>, this: This<'_, Self>) -> hebi::Result<()> {
        let message = scope.param::<Str>(0)?;

        this.0
            .send(Message::Text(to_irc_message(message.as_str().to_string())))
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

pub async fn run_hebi(
    irc_sender: Sender<Message>,
    alert_sender: broadcast::Sender<Alert>,
) -> eyre::Result<Hebi> {
    let mut vm = Hebi::new();

    let module = NativeModule::builder("ws")
        .class::<WsSender>("WsSender", |class| {
            class.async_method("send", WsSender::send).finish()
        })
        .class::<AlertSender>("AlertSender", |class| {
            class.async_method("send", AlertSender::send).finish()
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

    vm.eval_async(
        r#"
ws_sender.send("YEP")
        "#,
    )
    .await?;

    Ok(vm)
}
