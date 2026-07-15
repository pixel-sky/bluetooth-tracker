use crate::address::BluetoothAddress;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "keychron-tracker")]
#[command(about = "Track Bluetooth connection spans for configured devices")]
pub struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    pub state_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Discover,

    Watch {
        #[arg(long, required = true)]
        address: Vec<BluetoothAddress>,
    },

    Status {
        #[arg(long)]
        address: Vec<BluetoothAddress>,
    },

    Report {
        #[arg(long)]
        address: Vec<BluetoothAddress>,
    },

    Note {
        #[command(subcommand)]
        command: NoteCommands,
    },

    Service {
        #[command(subcommand)]
        command: ServiceCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum NoteCommands {
    Start {
        #[arg(long)]
        address: Option<BluetoothAddress>,

        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },

    End {
        #[arg(long)]
        address: Option<BluetoothAddress>,

        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ServiceCommands {
    Install {
        #[arg(long, required = true)]
        address: Vec<BluetoothAddress>,
    },

    Uninstall,
}
