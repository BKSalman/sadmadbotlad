use std::{convert::TryInto, fs, io::Read, sync::Arc};

use clap::{command, Parser};
use eyre::WrapErr;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use song_requests::Queue;
use tokio::task::JoinHandle;
use twitch::TwitchApiInfo;

pub mod commands;
pub mod db;
pub mod discord;
// pub mod event_handler;
pub mod eventsub;
pub mod irc;
pub mod obs_websocket;
pub mod song_requests;
pub mod sr_ws_server;
pub mod twitch;
pub mod ws_server;
pub mod youtube;

pub async fn flatten<T>(handle: JoinHandle<Result<T, eyre::Report>>) -> Result<T, eyre::Report> {
    match handle.await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(err),
        Err(e) => Err(e).wrap_err_with(|| "handling failed"),
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    #[arg(short, long, default_value_t = false)]
    pub manual: bool,
    #[arg(short, long, default_value_t = '!')]
    pub cmd_delim: char,
    #[arg(short, long, default_value_t = 3000)]
    pub port: u16,
}

#[derive(Debug)]
pub struct App {
    pub config: Config,
}

impl App {
    pub fn new() -> Self {
        Self {
            config: Config::parse(),
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

lazy_static! {
    pub static ref APP: App = App::new();
}

pub fn install_eyre() -> eyre::Result<()> {
    let (_, eyre_hook) = color_eyre::config::HookBuilder::default().into_hooks();

    eyre_hook.install()?;

    std::panic::set_hook(Box::new(move |pi| {
        eprintln!("{pi}");
    }));
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum AlertEventType {
    Follow {
        follower: String,
    },
    Raid {
        from: String,
        viewers: u64,
    },
    Subscribe {
        subscriber: String,
        tier: String,
    },
    ReSubscribe {
        subscriber: String,
        tier: String,
        subscribed_for: u64,
        streak: u64,
    },
    GiftSub {
        gifter: String,
        total: u64,
        tier: String,
    },
    GiftedSub {
        gifted: String,
        tier: String,
    },
    Bits {
        message: String,
        is_anonymous: bool,
        cheerer: String,
        bits: u64,
    },
}

impl From<AlertEventType> for surrealdb::sql::Value {
    fn from(value: AlertEventType) -> Self {
        match value {
            AlertEventType::Follow { follower } => {
                surrealdb::sql::Value::Object(surrealdb::sql::Object(collection! {
                    "type".into() => "Follow".into(),
                    "follower".into() => follower.into()
                }))
            }
            AlertEventType::Raid { from, viewers } => {
                surrealdb::sql::Value::Object(surrealdb::sql::Object(collection! {
                    "type".into() => "Raid".into(),
                    "from".into() => from.into(),
                    "viewers".into() => viewers.into(),
                }))
            }
            AlertEventType::Subscribe { subscriber, tier } => {
                surrealdb::sql::Value::Object(surrealdb::sql::Object(collection! {
                    "type".into() => "Subscribe".into(),
                    "subscriber".into() => subscriber.into(),
                    "tier".into() => tier.into(),
                }))
            }
            AlertEventType::ReSubscribe {
                subscriber,
                tier,
                subscribed_for,
                streak,
            } => surrealdb::sql::Value::Object(surrealdb::sql::Object(collection! {
                "type".into() => "ReSubscribe".into(),
                "subscriber".into() => subscriber.into(),
                "tier".into() => tier.into(),
                "subscribed_for".into() => subscribed_for.into(),
                "streak".into() => streak.into(),
            })),
            AlertEventType::GiftSub {
                gifter,
                total,
                tier,
            } => surrealdb::sql::Value::Object(surrealdb::sql::Object(collection! {
                "type".into() => "GiftSub".into(),
                "gifter".into() => gifter.into(),
                "total".into() => total.into(),
                "tier".into() => tier.into(),
            })),
            AlertEventType::GiftedSub { gifted, tier } => {
                surrealdb::sql::Value::Object(surrealdb::sql::Object(collection! {
                    "type".into() => "GiftedSub".into(),
                    "gifted".into() => gifted.into(),
                    "tier".into() => tier.into(),
                }))
            }
            AlertEventType::Bits {
                is_anonymous,
                cheerer,
                bits,
                message,
            } => surrealdb::sql::Value::Object(surrealdb::sql::Object(collection! {
                "type".into() => "Bits".into(),
                "cheerer".into() => if is_anonymous {
                    "Anonymous".into()
                } else {
                    cheerer.into()
                },
                "bits".into() => bits.into(),
                "message".into() => message.into(),
            })),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Alert {
    new: bool,
    r#type: AlertEventType,
}

#[derive(Debug, Clone)]
pub enum SrEvent {
    QueueRequest,
    QueueResponse(Arc<tokio::sync::RwLock<Queue>>),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiInfo {
    pub google_api_key: String,
    #[serde(flatten)]
    pub twitch: TwitchApiInfo,
    pub discord_token: String,
    pub obs_server_password: String,
}

impl ApiInfo {
    pub async fn new() -> eyre::Result<Self> {
        let Ok(mut config) = fs::File::open("config.toml") else {
            panic!("no config file");
        };

        let mut config_str = String::new();
        config.read_to_string(&mut config_str).expect("config str");

        Ok(toml::from_str::<ApiInfo>(&config_str)?)
    }
}

// lazy man's macros

#[macro_export]
macro_rules! collection {
    // map-like
    ($($k:expr => $v:expr),* $(,)?) => {{
        core::convert::From::from([$(($k, $v),)*])
    }};
    // set-like
    ($($v:expr),* $(,)?) => {{
        core::convert::From::from([$($v,)*])
    }};
}

#[macro_export]
macro_rules! string {
    ($s: expr) => {{
        String::from($s)
    }};
}

// This allows me to implement traits on external types
// it's called Newtype pattern
pub struct Wrapper<T>(pub T);

use surrealdb::sql::{Array, Object, Value};

impl TryFrom<Wrapper<Value>> for Object {
    type Error = eyre::Report;
    fn try_from(val: Wrapper<Value>) -> eyre::Result<Object> {
        match val.0 {
            Value::Object(obj) => Ok(obj),
            _ => Err(eyre::eyre!("value not of type Object")),
        }
    }
}

impl TryFrom<Wrapper<Value>> for Array {
    type Error = eyre::Report;
    fn try_from(val: Wrapper<Value>) -> eyre::Result<Array> {
        match val.0 {
            Value::Array(obj) => Ok(obj),
            _ => Err(eyre::eyre!("value not of type Array")),
        }
    }
}

impl TryFrom<Wrapper<Value>> for i64 {
    type Error = eyre::Report;
    fn try_from(val: Wrapper<Value>) -> eyre::Result<i64> {
        match val.0 {
            Value::Number(obj) => Ok(obj.as_int()),
            _ => Err(eyre::eyre!("value not of type i64")),
        }
    }
}

impl TryFrom<Wrapper<Value>> for bool {
    type Error = eyre::Report;
    fn try_from(val: Wrapper<Value>) -> eyre::Result<bool> {
        match val.0 {
            Value::False => Ok(false),
            Value::True => Ok(true),
            _ => Err(eyre::eyre!("value not of type bool")),
        }
    }
}

impl TryFrom<Wrapper<Value>> for String {
    type Error = eyre::Report;
    fn try_from(val: Wrapper<Value>) -> eyre::Result<String> {
        match val.0 {
            Value::Strand(strand) => Ok(strand.as_string()),
            Value::Thing(thing) => Ok(thing.to_string()),
            _ => Err(eyre::eyre!("value not of type String")),
        }
    }
}

pub trait TakeImpl<T> {
    fn take_impl(&mut self, key: &str) -> eyre::Result<Option<T>>;
}

impl TakeImpl<String> for Object {
    fn take_impl(&mut self, key: &str) -> eyre::Result<Option<String>> {
        let value = self.remove(key).map(|v| Wrapper(v).try_into());
        match value {
            None => Ok(None),
            Some(Ok(value)) => Ok(Some(value)),
            Some(Err(ex)) => Err(ex),
        }
    }
}

impl TakeImpl<bool> for Object {
    fn take_impl(&mut self, key: &str) -> eyre::Result<Option<bool>> {
        Ok(self.remove(key).map(|v| v.is_true()))
    }
}

impl TakeImpl<i64> for Object {
    fn take_impl(&mut self, key: &str) -> eyre::Result<Option<i64>> {
        let value = self.remove(key).map(|v| Wrapper(v).try_into());
        match value {
            None => Ok(None),
            Some(Ok(value)) => Ok(Some(value)),
            Some(Err(ex)) => Err(ex),
        }
    }
}

pub trait TakeVal {
    fn take_val<T>(&mut self, key: &str) -> eyre::Result<T>
    where
        Self: TakeImpl<T>;
}

impl<S> TakeVal for S {
    fn take_val<T>(&mut self, key: &str) -> eyre::Result<T>
    where
        Self: TakeImpl<T>,
    {
        let value: Option<T> = TakeImpl::take_impl(self, key)?;
        value.ok_or_else(|| eyre::eyre!("Property {} not found ", key))
    }
}
