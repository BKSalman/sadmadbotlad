use tokio::sync::mpsc::UnboundedSender;

use crate::event_handler::Event;

use super::Command;

struct Ping {
    // ws_sender: ,
}

impl Command for Ping {
    fn execute(&self, sender: UnboundedSender<Event>) -> eyre::Result<()> {
        Ok(())
    }
}
