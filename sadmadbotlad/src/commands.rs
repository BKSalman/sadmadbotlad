use std::{fs, sync::Arc};

use hebi::{Hebi, IntoValue, NativeModule, Scope, Str, This};
use libmpv::Mpv;
use tokio::sync::{
    broadcast,
    mpsc::{self, Sender},
    oneshot,
};
use tokio_tungstenite::tungstenite::Message;

use crate::{
    irc::{to_irc_message, Tags, TwitchIrcMessage},
    song_requests::{QueueMessages, SongRequest},
    twitch::TwitchTokenMessages,
    Alert, APP,
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
        // TODO: handle this inside hebi
        // if args.is_empty() {
        //     let e = format!("Correct usage: {}sr <URL>", APP.config.cmd_delim);
        //     ws_sender.send(Message::Text(to_irc_message(&e))).await?;
        //     continue;
        // }

        let args = scope.param::<Str>(0)?;
        let sender = scope.param::<Str>(1)?;

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
    // TODO: handle inside hebi
    // async fn skip(scope: Scope<'_>, this: This<'_, Self>) -> hebi::Result<Option<SongRequest>> {
    //     // if let Some(message) = mods_only(&parsed_msg)? {
    //     //     ws_sender
    //     //         .send(Message::Text(to_irc_message(&message)))
    //     //         .await?;
    //     //     continue;
    //     // }

    //     let Ok(current_song) = Self::get_current_song(scope, this).await else {
    //         return Err(SongRequestsError::CouldNotGetCurrentSong).map_err(hebi::Error::user);
    //     };

    //     // TODO: handle inside hebi

    //     // let mut chat_message = String::new();

    //     // if let Some(song) = current_song {
    //     //     if mpv.0.playlist_next_force().is_ok() {
    //     //         chat_message = format!("Skipped: {}", song.title);
    //     //     }
    //     // } else {
    //     //     chat_message = String::from("No song playing");
    //     // }

    //     Ok(())
    // }
    async fn get_current_song<'a>(
        scope: Scope<'a>,
        this: This<'_, Self>,
    ) -> hebi::Result<Option<hebi::Value<'a>>> {
        let (send, recv) = oneshot::channel();

        this.0
            .send(QueueMessages::GetCurrentSong(send))
            .map_err(hebi::Error::user)?;

        let Some(current_song) = recv.await.map_err(hebi::Error::user)? else {
            return Ok(None);
        };

        let current_song = scope.new_instance(current_song)?;

        Ok(Some(current_song))
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

#[derive(thiserror::Error, Debug)]
enum MpvClientError {
    #[error("{0}")]
    MpvError(String),
}

struct MpvClient(Arc<Mpv>);

impl MpvClient {
    fn next(_scope: Scope<'_>, this: This<'_, Self>) -> hebi::Result<()> {
        if let Err(err) = this.0.playlist_next_force() {
            eprintln!("Mpv Error: {:#?}", err);
            return Err(MpvClientError::MpvError(err.to_string())).map_err(hebi::Error::user);
        }

        Ok(())
    }
    fn get_volume(_scope: Scope<'_>, this: This<'_, Self>) -> hebi::Result<i32> {
        match this.0.get_property::<i64>("volume") {
            Ok(volume) => Ok(volume as i32),
            Err(err) => {
                eprintln!("Mpv Error: {:#?}", err);
                Err(MpvClientError::MpvError(err.to_string())).map_err(hebi::Error::user)
            }
        }
    }
    fn set_volume(scope: Scope<'_>, this: This<'_, Self>) -> hebi::Result<()> {
        let volume = scope.param::<i32>(0)?;

        if let Err(err) = this.0.set_property("volume", volume as i64) {
            eprintln!("Mpv Error: {:#?}", err);
            return Err(MpvClientError::MpvError(err.to_string())).map_err(hebi::Error::user);
        }

        Ok(())
    }
}

pub struct Context {
    pub args: Vec<String>,
    pub message_metadata: TwitchIrcMessage,
}

impl Context {
    fn args<'a>(scope: Scope<'a>, this: This<'_, Self>) -> hebi::Result<hebi::List<'a>> {
        let args = scope.new_list(this.args.capacity());
        for arg in this.args.iter() {
            let arg = scope.new_string(arg);
            args.push(arg.into_value(scope.global())?);
        }

        Ok(args)
    }

    fn set_working_on<'a>(scope: Scope<'a>, _this: This<'_, Self>) -> hebi::Result<()> {
        let args = scope.param::<Str>(0)?;

        fs::write("workingon.txt", format!("Currently: {}", args)).map_err(hebi::Error::user)?;

        Ok(())
    }

    fn get_working_on<'a>(_scope: Scope<'a>, _this: This<'_, Self>) -> hebi::Result<String> {
        let working_on = fs::read_to_string("workingon.txt")
            .map_err(hebi::Error::user)?
            .split_once(" ")
            .unwrap_or(("", ""))
            .1
            .to_string();

        Ok(working_on)
    }

    fn message_metadata<'a>(
        scope: Scope<'a>,
        this: This<'_, Self>,
    ) -> hebi::Result<hebi::Table<'a>> {
        let metadata = scope.new_table(
            this.message_metadata.tags.capacity() + this.message_metadata.message.capacity(),
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
    }
}

pub async fn run_hebi(
    irc_sender: Sender<Message>,
    alert_sender: broadcast::Sender<Alert>,
    token_sender: mpsc::UnboundedSender<TwitchTokenMessages>,
    mpv: Arc<Mpv>,
    queue_sender: mpsc::UnboundedSender<QueueMessages>,
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

    vm.global().set(
        vm.new_string("cmd_delim"),
        vm.new_string(APP.config.cmd_delim)
            .into_value(vm.global())?,
    );

    vm.global().set(
        vm.new_string("song_request_client"),
        vm.new_instance(SongRequetsClient(queue_sender))?,
    );

    vm.global()
        .set(vm.new_string("mpv"), vm.new_instance(MpvClient(mpv))?);

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
                .method("args", Context::args)
                .method("message_metadata", Context::message_metadata)
                .method("set_working_on", Context::set_working_on)
                .method("get_working_on", Context::get_working_on)
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
        .class::<SongRequetsClient>("SongRequetsClient", |class| {
            class
                .async_method("sr", SongRequetsClient::sr)
                .async_method("get_current_song", SongRequetsClient::get_current_song)
                .finish()
        })
        .class::<MpvClient>("MpvClient", |class| {
            class
                .method("next", MpvClient::next)
                .method("set_volume", MpvClient::set_volume)
                .method("get_volume", MpvClient::get_volume)
                .finish()
        })
        .class::<SongRequest>("SongRequest", |class| {
            class
                .method("title", |_scope, this| this.title.clone())
                .method("url", |_scope, this| this.url.clone())
                .method("user", |_scope, this| this.user.clone())
                .method("id", |_scope, this| this.id.clone())
                .finish()
        })
        .finish()
}
