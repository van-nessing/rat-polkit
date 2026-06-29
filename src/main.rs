use clap::Parser;
use color_eyre::Result;

use crate::{handler::HandlerArgs, tui::TuiArgs};

mod agent;
mod authority;
mod handler;
mod messages;
mod polkit_helper;
mod tui;

#[derive(Parser)]
pub enum Command {
    Tui(TuiArgs),
    Handler(HandlerArgs),
}

fn main() -> Result<()> {
    let command = Command::parse();
    let _ = color_eyre::install();
    match command {
        Command::Tui(args) => tui::run(args),
        Command::Handler(args) => handler::run(args),
    }
}
