#![allow(unused)]

use std::collections::HashMap;

use crate::{collection, string};

#[derive(Debug)]
struct Command {
    name: String,
    shorthand: String,
}

trait CommandRunner {
    fn run(&self) -> Result<(), eyre::Report>;
}

impl CommandRunner for Command {
    fn run(&self) -> Result<(), eyre::Report> {
        println!("lmao");
        Ok(())
    }
}

pub fn something() {
    let something = Command {
        name: String::from("idk"),
        shorthand: String::from("idk"),
    };

    // let mut commands: HashMap<String, Command> =
    //     HashMap::from([(something.name.clone(), something)]);

    let commands: HashMap<String, Command> = collection! {
        string!("something") => something,
    };

    println!("{commands:#?}");

    // let received_cmd = String::from("idk");

    // if let Some(called_cmd) = commands.get(&received_cmd) {
    //     called_cmd.run().expect("something");
    // }
}
