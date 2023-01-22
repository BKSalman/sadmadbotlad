use tokio::sync::mpsc::UnboundedSender;

use crate::event_handler::Event;

trait Command {
    fn execute(&self, sender: UnboundedSender<Event>) -> eyre::Result<()>;
}

fn execute(command: impl Command, sender: UnboundedSender<Event>) -> eyre::Result<()> {
    command.execute(sender)?;

    Ok(())
}
