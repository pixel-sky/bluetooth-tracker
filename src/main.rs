mod address;
mod bluez;
mod cli;
mod display;
mod notes;
mod paths;
mod report;
mod service;
mod storage;
mod tracking;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands, NoteCommands, ServiceCommands};
use paths::TrackerPaths;
use storage::SpanBoundary;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = TrackerPaths::from_overrides(cli.log, cli.state)?;

    match cli.command {
        Commands::Discover => bluez::discover().await,
        Commands::Watch { address } => tracking::watch(paths, address).await,
        Commands::Status { address } => tracking::status(paths, address).await,
        Commands::Report => report::print_report(&paths),
        Commands::Note { command } => match command {
            NoteCommands::Start { text } => notes::add_note(&paths, SpanBoundary::Start, &text),
            NoteCommands::End { text } => notes::add_note(&paths, SpanBoundary::End, &text),
        },
        Commands::Service { command } => match command {
            ServiceCommands::Install { address } => service::install(&address, &paths),
            ServiceCommands::Uninstall => service::uninstall(),
        },
    }
}
