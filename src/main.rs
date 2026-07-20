mod address;
mod battery;
mod bluez;
mod cli;
mod display;
mod notes;
mod paths;
mod report;
mod service;
mod storage;
mod storage_jsonl;
mod storage_lock;
mod tracking;

use anyhow::Result;
use clap::Parser;
use cli::{BatteryCommands, Cli, Commands, NoteCommands, ServiceCommands};
use paths::TrackerPaths;
use storage::SpanBoundary;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = match cli.state_dir {
        Some(state_dir) => TrackerPaths::new(state_dir),
        None => TrackerPaths::from_default_state_dir()?,
    };

    match cli.command {
        Commands::Discover => bluez::discover().await,
        Commands::Watch { address } => tracking::watch(paths, address).await,
        Commands::Status { address } => tracking::status(paths, address).await,
        Commands::Report { address } => report::print_report(&paths, &address),
        Commands::Note { command } => match command {
            NoteCommands::Start { address, text } => {
                notes::add_note(&paths, address.as_ref(), SpanBoundary::Start, &text)
            }
            NoteCommands::End { address, text } => {
                notes::add_note(&paths, address.as_ref(), SpanBoundary::End, &text)
            }
        },
        Commands::Battery { command } => match command {
            BatteryCommands::Set {
                address,
                percentage,
            } => battery::set(&paths, address.as_ref(), percentage),
        },
        Commands::Service { command } => match command {
            ServiceCommands::Install { address } => service::install(&address, &paths),
            ServiceCommands::Uninstall => service::uninstall(),
        },
    }
}
