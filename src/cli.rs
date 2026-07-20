use crate::address::BluetoothAddress;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "keychron-tracker")]
#[command(about = "Track Bluetooth connection spans and battery levels for configured devices")]
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

    Battery {
        #[command(subcommand)]
        command: BatteryCommands,
    },

    Service {
        #[command(subcommand)]
        command: ServiceCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum BatteryCommands {
    Set {
        #[arg(long)]
        address: Option<BluetoothAddress>,

        #[arg(value_parser = clap::value_parser!(u8).range(0..=100))]
        percentage: u8,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_set_parses_address_and_percentage() {
        let cli = Cli::try_parse_from([
            "keychron-tracker",
            "battery",
            "set",
            "--address",
            "aa:bb:cc:dd:ee:ff",
            "55",
        ])
        .unwrap();

        let Commands::Battery {
            command:
                BatteryCommands::Set {
                    address,
                    percentage,
                },
        } = cli.command
        else {
            panic!("expected battery set command");
        };
        assert_eq!(
            address.as_ref().map(BluetoothAddress::as_str),
            Some("AA:BB:CC:DD:EE:FF")
        );
        assert_eq!(percentage, 55);
    }

    #[test]
    fn battery_set_parses_percentage_without_address() {
        let cli = Cli::try_parse_from(["keychron-tracker", "battery", "set", "55"]).unwrap();

        let Commands::Battery {
            command:
                BatteryCommands::Set {
                    address,
                    percentage,
                },
        } = cli.command
        else {
            panic!("expected battery set command");
        };
        assert_eq!(address, None);
        assert_eq!(percentage, 55);
    }

    #[test]
    fn battery_set_rejects_percentage_above_one_hundred() {
        assert!(
            Cli::try_parse_from([
                "keychron-tracker",
                "battery",
                "set",
                "--address",
                "AA:BB:CC:DD:EE:FF",
                "101",
            ])
            .is_err()
        );
    }
}
