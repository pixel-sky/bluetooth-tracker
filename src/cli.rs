use crate::address::BluetoothAddress;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "keychron-tracker")]
#[command(about = "Track Bluetooth connection spans for a Keychron keyboard")]
pub struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    pub log: Option<PathBuf>,

    #[arg(long, global = true, value_name = "PATH")]
    pub state: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Discover,

    Watch {
        #[arg(long)]
        address: BluetoothAddress,
    },

    Status {
        #[arg(long)]
        address: BluetoothAddress,
    },

    Report,

    Service {
        #[command(subcommand)]
        command: ServiceCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum ServiceCommands {
    Install {
        #[arg(long)]
        address: BluetoothAddress,
    },

    Uninstall,
}
