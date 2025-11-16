use clap::Parser;
use hebi::prelude::*;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io::Read, path::PathBuf, sync::Arc};
use tokio::task::JoinHandle;
use tracing::{Level, metadata::LevelFilter};
use tracing_appender::rolling;
use tracing_subscriber::{
    EnvFilter, fmt::writer::MakeWriterExt, prelude::__tracing_subscriber_SubscriberExt,
    util::SubscriberInitExt,
};

use song_requests::Queue;
use twitch::TwitchApiInfo;

pub mod commands;
pub mod db;
pub mod discord;
pub mod eventsub;
pub mod irc;
pub mod obs_websocket;
pub mod song_requests;
pub mod sr_ws_server;
pub mod twitch;
pub mod ws_server;
pub mod youtube;

pub async fn flatten<T>(handle: JoinHandle<anyhow::Result<T>>) -> anyhow::Result<T> {
    match handle.await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(err),
        Err(e) => Err(e.into()),
    }
}

pub fn oneshot<T>() -> (OneShotSender<T>, OneShotReceiver<T>) {
    let (tx, rx) = std::sync::mpsc::channel();

    (OneShotSender(tx), OneShotReceiver(rx))
}

pub struct OneShotSender<T>(std::sync::mpsc::Sender<T>);

impl<T> OneShotSender<T> {
    pub fn send(self, t: T) -> Result<(), std::sync::mpsc::SendError<T>> {
        self.0.send(t)
    }
}

pub struct OneShotReceiver<T>(std::sync::mpsc::Receiver<T>);

impl<T> OneShotReceiver<T> {
    pub fn recv(self) -> Result<T, std::sync::mpsc::RecvError> {
        self.0.recv()
    }
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[arg(long)]
    pub db: Option<PathBuf>,
    #[arg(short = 'c', long)]
    pub config_path: Option<PathBuf>,
    #[arg(short = 'o', long)]
    pub commands_path: Option<PathBuf>,
    #[arg(short = 'd', long)]
    pub cmd_delim: Option<char>,
    #[arg(short, long)]
    pub port: Option<u16>,
    #[arg(short, long)]
    pub frontend_port: Option<u16>,
    #[arg(short, long)]
    pub static_path: Option<PathBuf>,
}

#[derive(Debug)]
pub struct Config {
    pub database_path: PathBuf,
    pub config_path: PathBuf,
    pub commands_path: PathBuf,
    pub cmd_delim: char,
    pub port: u16,
    pub frontend_port: u16,
    pub static_path: PathBuf,
}

#[derive(Debug)]
pub struct App {
    pub config: Config,
}

impl App {
    pub fn new() -> Self {
        let flags = Cli::parse();

        Self {
            config: Config {
                database_path: flags.db.unwrap_or("database.db".into()),
                config_path: flags.config_path.unwrap_or("./config.toml".into()),
                commands_path: flags.commands_path.unwrap_or("./commands".into()),
                cmd_delim: flags.cmd_delim.unwrap_or('!'),
                port: flags.port.unwrap_or(3000),
                frontend_port: flags.frontend_port.unwrap_or(8080),
                static_path: flags.static_path.unwrap_or_default(),
            },
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Alert {
    new: bool,
    r#type: AlertEventType,
}

impl Alert {
    pub fn raid_test() -> Self {
        Alert {
            new: true,
            r#type: AlertEventType::Raid {
                from: String::from("lmao"),
                viewers: 9999,
            },
        }
    }
    pub fn follow_test() -> Self {
        Alert {
            new: true,
            r#type: AlertEventType::Follow {
                follower: String::from("lmao"),
            },
        }
    }
    pub fn sub_test() -> Self {
        Alert {
            new: true,
            r#type: AlertEventType::Subscribe {
                subscriber: String::from("lmao"),
                tier: String::from("3"),
            },
        }
    }
    pub fn resub_test() -> Self {
        Alert {
            new: true,
            r#type: AlertEventType::ReSubscribe {
                subscriber: String::from("lmao"),
                tier: String::from("3"),
                subscribed_for: 4,
                streak: 2,
            },
        }
    }
    pub fn giftsub_test() -> Self {
        Alert {
            new: true,
            r#type: AlertEventType::GiftSub {
                gifter: String::from("lmao"),
                total: 9999,
                tier: String::from("3"),
            },
        }
    }
    pub fn asd_test() -> Self {
        Alert {
            new: true,
            r#type: AlertEventType::Raid {
                from: String::from("asd"),
                viewers: 9999,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub enum SrEvent {
    QueueRequest,
    QueueResponse(Arc<tokio::sync::RwLock<Queue>>),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ApiInfo {
    pub google_api_key: String,
    #[serde(flatten)]
    pub twitch: TwitchApiInfo,
    pub discord_token: String,
    pub obs_server_password: String,
}

impl ApiInfo {
    pub fn new() -> anyhow::Result<Self> {
        let Ok(mut config) = fs::File::open(&APP.config.config_path) else {
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
    ($s: expr) => {{ String::from($s) }};
}

#[derive(Default, Debug)]
pub struct CommandsLoader {
    commands: HashMap<String, String>,
}

#[derive(thiserror::Error, Debug)]
pub enum CommandsError {
    #[error("invalid file: should be a hebi file with .hebi extension")]
    InvalidFile,

    #[error("file name is invalid")]
    InvalidFileName,

    #[error(transparent)]
    IoError(#[from] std::io::Error),
}

impl CommandsLoader {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    pub fn load_commands(path: &PathBuf) -> Result<HashMap<String, String>, CommandsError> {
        tracing::info!("loading commands");

        let mut commands = HashMap::new();

        for file in fs::read_dir(path)? {
            let file = file?;

            let file_name = file
                .file_name()
                .to_str()
                .ok_or(CommandsError::InvalidFileName)?
                .to_string();

            let Some((command_name, extension)) = file_name.rsplit_once('.') else {
                return Err(CommandsError::InvalidFile);
            };

            let command_aliases = command_name.split('+');

            let command_aliases: Vec<(String, String)> = command_aliases
                .flat_map(|name| {
                    // tracing::info!("command name: {name} -- extension: {extension}");
                    Ok::<_, std::io::Error>((
                        name.to_string(),
                        fs::read_to_string(path.join(file_name.clone()))?,
                    ))
                })
                .collect();

            if command_aliases.is_empty() {
                tracing::info!("command name: {command_name} -- extension: {extension}");

                commands.insert(
                    command_name.to_string(),
                    fs::read_to_string(path.join(file_name))?,
                );

                continue;
            }

            for (command_name, command_code) in command_aliases {
                commands.insert(command_name, command_code);
            }
        }

        Ok(commands)
    }
}

impl ModuleLoader for CommandsLoader {
    fn load(&self, path: &str) -> hebi::Result<hebi::Cow<'static, str>> {
        Ok(hebi::Cow::owned(
            self.commands
                .get(path)
                .ok_or_else(|| hebi::error!("failed to load module {path}"))?
                .clone(),
        ))
    }
}

pub fn logging() {
    if let Err(_e) = std::env::var("RUST_LOG") {
        println!("no log level specified, defaulting to debug level for sadmadbotlad crate only");
        unsafe { std::env::set_var("RUST_LOG", "none,sadmadbotlad=debug") };
    }

    let logs_dir = dirs::cache_dir()
        .expect("cache dir should exist")
        .join("sadmadbotlad/logs");

    // Log all `tracing` events to files prefixed with `debug`. Since these
    // files will be written to very frequently, roll the log file every hour.
    let debug_file = rolling::hourly(&logs_dir, "debug");
    // Log warnings and errors to a separate file. Since we expect these events
    // to occur less frequently, roll that file on a daily basis instead.
    let warn_file = rolling::daily(&logs_dir, "warnings");

    tracing_subscriber::registry()
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::ERROR.into())
                .from_env_lossy(),
        )
        .with(
            tracing_subscriber::fmt::Layer::default()
                .with_writer(debug_file.with_max_level(Level::DEBUG))
                .with_ansi(false),
        )
        .with(
            tracing_subscriber::fmt::Layer::default()
                .with_writer(warn_file.with_max_level(tracing::Level::WARN))
                .with_ansi(false),
        )
        .with(
            tracing_subscriber::fmt::Layer::default()
                .with_writer(std::io::stdout.with_max_level(Level::DEBUG)),
        )
        .init();
}
