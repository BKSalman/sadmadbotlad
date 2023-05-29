use std::fs;

use hebi::{Hebi, IntoValue, NativeModule, Scope, Str, This};
use tokio::sync::{
    broadcast,
    mpsc::{self, Sender},
    oneshot,
};
use tokio_tungstenite::tungstenite::Message;

use crate::{
    irc::{to_irc_message, Tags, TwitchIrcMessage},
    song_requests::QueueMessages,
    twitch::TwitchTokenMessages,
    Alert,
};

const RUST_WARRANTY: &str = include_str!("../rust_warranty.txt");

const WARRANTY: &str = include_str!("../warranty.txt");

const RULES: &str = include_str!("../rules.txt");

struct WsSender(Sender<Message>);

impl WsSender {
    async fn send(scope: Scope<'_>, this: This<'_, Self>) -> hebi::Result<()> {
        let message = scope.param::<Str>(0)?;

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
struct SongRequetsClient(mpsc::UnboundedSender<QueueMessages>);

impl SongRequetsClient {
    async fn sr(scope: Scope<'_>, this: This<'_, Self>) -> hebi::Result<String> {
        // if args.is_empty() {
        //     let e = format!("Correct usage: {}sr <URL>", APP.config.cmd_delim);
        //     ws_sender.send(Message::Text(to_irc_message(&e))).await?;
        //     continue;
        // }

        let args = scope.param::<Str>(0)?;
        let sender = scope.param::<Str>(0)?;

        let (send, recv) = oneshot::channel();

        this.0
            .send(QueueMessages::Sr(
                (sender.to_string(), args.to_string()),
                send,
            ))
            .map_err(hebi::Error::user)?;

        let chat_message = recv.await.map_err(hebi::Error::user)?;

        Ok(chat_message)
    }
    // async fn skip(scope: Scope<'_>, this: This<'_, Self>) -> hebi::Result<()> {
    //     // if let Some(message) = mods_only(&parsed_msg)? {
    //     //     ws_sender
    //     //         .send(Message::Text(to_irc_message(&message)))
    //     //         .await?;
    //     //     continue;
    //     // }

    //     let mpv = scope.param::<WsSender>(0)?;

    //     let (send, recv) = oneshot::channel();

    //     this.0.send(QueueMessages::GetCurrentSong(send))?;

    //     let Ok(current_song) = recv.await else {
    //         return Err(SongRequestsError::CouldNotGetCurrentSong).map_err(hebi::Error::user);
    //     };

    //     let mut chat_message = String::new();

    //     if let Some(song) = current_song {
    //         if mpv.playlist_next_force().is_ok() {
    //             chat_message = format!("Skipped: {}", song.title);
    //         }
    //     } else {
    //         chat_message = String::from("No song playing");
    //     }
    //     Ok(())
    // }
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

pub struct Context {
    pub args: Vec<String>,
    pub message_metadata: TwitchIrcMessage,
}

pub async fn run_hebi(
    irc_sender: Sender<Message>,
    alert_sender: broadcast::Sender<Alert>,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
) -> Result<Hebi, hebi::Error> {
    let mut vm = Hebi::new();

    let module = get_module();

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

    vm.global().set(
        vm.new_string("rust_warranty"),
        vm.new_string(RUST_WARRANTY).into_value(vm.global())?,
    );

    vm.global().set(
        vm.new_string("warranty"),
        vm.new_string(WARRANTY).into_value(vm.global())?,
    );

    vm.global().set(
        vm.new_string("rules"),
        vm.new_string(RULES).into_value(vm.global())?,
    );

    vm.eval_async(
        r#"
ws_sender.send("YEP")
        "#,
    )
    .await?;

    Ok(vm)
}

fn get_module() -> NativeModule {
    NativeModule::builder("ws")
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
        .class::<Context>("Context", |class| {
            class
                .method("args", |scope, this| {
                    let args = scope.new_list(this.args.capacity());
                    for arg in this.args.iter() {
                        let arg = scope.new_string(arg);
                        args.push(arg.into_value(scope.global())?);
                    }

                    Ok(args)
                })
                .method("message_metadata", |scope, this| {
                    let metadata = scope.new_table(
                        this.message_metadata.tags.capacity()
                            + this.message_metadata.message.capacity(),
                    );

                    let tags = scope.new_instance(this.message_metadata.tags.clone())?;

                    metadata.insert(scope.new_string("tags"), tags);

                    metadata.insert(
                        scope.new_string("message"),
                        this.message_metadata
                            .message
                            .clone()
                            .into_value(scope.global())?,
                    );

                    Ok(metadata)
                })
                .method("set_working_on", |scope, _this| {
                    let args = scope.param::<Str>(0)?;

                    fs::write("workingon.txt", format!("Currently: {}", args))
                        .map_err(hebi::Error::user)?;

                    Ok(())
                })
                .method("get_working_on", |_scope, _this| {
                    let working_on = fs::read_to_string("workingon.txt")
                        .map_err(hebi::Error::user)?
                        .split_once(" ")
                        .unwrap_or(("", ""))
                        .1
                        .to_string();

                    Ok(working_on)
                })
                .finish()
        })
        .class::<TwitchIrcMessage>("TwitchIrcMessage", |class| class.finish())
        .class::<Tags>("Tags", |class| {
            class
                .method("mods_only", |_scope, this| this.mods_only())
                .method("get_reply", |_scope, this| this.get_reply())
                .method("get_sender", |_scope, this| this.get_sender())
                .finish()
        })
        .finish()
}
