use async_trait::async_trait;
use futures_util::{stream::SplitSink, SinkExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::{irc::to_irc_message, Alert, AlertEventType};

use super::Command;

pub struct AlertTestCommand {
    alert_sender: tokio::sync::broadcast::Sender<Alert>,
    test: String,
}

impl AlertTestCommand {
    pub fn new(alert_sender: tokio::sync::broadcast::Sender<Alert>, test: String) -> Self {
        Self { alert_sender, test }
    }
}

#[async_trait]
impl Command for AlertTestCommand {
    async fn execute(
        &self,
        ws_sender: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
    ) -> eyre::Result<()> {
        let alert = match self.test.to_lowercase().as_str() {
            "raid" => Alert {
                new: true,
                r#type: AlertEventType::Raid {
                    from: String::from("lmao"),
                    viewers: 9999,
                },
            },
            "follow" => Alert {
                new: true,
                r#type: AlertEventType::Follow {
                    follower: String::from("lmao"),
                },
            },
            "sub" => Alert {
                new: true,
                r#type: AlertEventType::Subscribe {
                    subscriber: String::from("lmao"),
                    tier: String::from("3"),
                },
            },
            "resub" => Alert {
                new: true,
                r#type: AlertEventType::ReSubscribe {
                    subscriber: String::from("lmao"),
                    tier: String::from("3"),
                    subscribed_for: 4,
                    streak: 2,
                },
            },
            "giftsub" => Alert {
                new: true,
                r#type: AlertEventType::GiftSub {
                    gifter: String::from("lmao"),
                    total: 9999,
                    tier: String::from("3"),
                },
            },
            _ => {
                println!("no args provided");
                Alert {
                    new: true,
                    r#type: AlertEventType::Raid {
                        from: String::from("lmao"),
                        viewers: 9999,
                    },
                }
            }
        };

        match self.alert_sender.send(alert) {
            Ok(_) => {
                ws_sender
                    .send(Message::Text(to_irc_message(&format!(
                        "Testing {} alert",
                        if self.test.is_empty() {
                            "raid"
                        } else {
                            &self.test
                        }
                    ))))
                    .await?;
            }
            Err(e) => {
                println!("frontend event failed:: {e:?}");
            }
        }

        Ok(())
    }
}
