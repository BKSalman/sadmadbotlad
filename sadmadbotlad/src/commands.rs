struct SomeCommand {
    name: String,
    shorthand: String,
}

trait Command {
    fn run() -> Result<(), eyre::Report>;
}
