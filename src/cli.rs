use crate::address::BluetoothAddress;
use clap::{Parser, Subcommand};
use clap_complete::Shell;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "bluetooth-tracker")]
#[command(about = "Track Bluetooth connection spans and battery levels for configured devices")]
pub struct Cli {
    /// Use a custom directory for tracker state files
    #[arg(long, global = true, value_name = "PATH")]
    pub state_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Discover nearby Bluetooth devices
    Discover,

    /// Track connection and battery changes
    Watch {
        /// Bluetooth address to track; may be repeated
        #[arg(long, required = true)]
        address: Vec<BluetoothAddress>,
    },

    /// Show the current state of tracked devices
    Status {
        /// Limit output to this Bluetooth address; may be repeated
        #[arg(long)]
        address: Vec<BluetoothAddress>,
    },

    /// Report connection spans and battery observations
    Report {
        /// Limit output to this Bluetooth address; may be repeated
        #[arg(long)]
        address: Vec<BluetoothAddress>,
    },

    /// Add notes to connection spans
    Note {
        #[command(subcommand)]
        command: NoteCommands,
    },

    /// Record battery percentages manually
    Battery {
        #[command(subcommand)]
        command: BatteryCommands,
    },

    /// Manage the user-level systemd service
    Service {
        #[command(subcommand)]
        command: ServiceCommands,
    },

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
}

#[derive(Debug, Subcommand)]
pub enum BatteryCommands {
    /// Record a battery percentage
    Set {
        /// Bluetooth address of the span to update
        #[arg(long)]
        address: Option<BluetoothAddress>,

        /// Battery percentage from 0 to 100
        #[arg(value_parser = clap::value_parser!(u8).range(0..=100))]
        percentage: u8,
    },
}

#[derive(Debug, Subcommand)]
pub enum NoteCommands {
    /// Add a note to the start of a span
    Start {
        /// Bluetooth address of the span to update
        #[arg(long)]
        address: Option<BluetoothAddress>,

        /// Words to store in the note
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },

    /// Add a note to the end of a span
    End {
        /// Bluetooth address of the span to update
        #[arg(long)]
        address: Option<BluetoothAddress>,

        /// Words to store in the note
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ServiceCommands {
    /// Install and start the tracker service
    Install {
        /// Bluetooth address to track; may be repeated
        #[arg(long, required = true)]
        address: Vec<BluetoothAddress>,
    },

    /// Stop and remove the tracker service
    Uninstall,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_set_parses_address_and_percentage() {
        let cli = Cli::try_parse_from([
            "bluetooth-tracker",
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
        let cli = Cli::try_parse_from(["bluetooth-tracker", "battery", "set", "55"]).unwrap();

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
                "bluetooth-tracker",
                "battery",
                "set",
                "--address",
                "AA:BB:CC:DD:EE:FF",
                "101",
            ])
            .is_err()
        );
    }

    #[test]
    fn completions_parses_supported_shell() {
        let cli = Cli::try_parse_from(["bluetooth-tracker", "completions", "zsh"]).unwrap();

        let Commands::Completions { shell } = cli.command else {
            panic!("expected completions command");
        };
        assert_eq!(shell, Shell::Zsh);
    }

    #[test]
    fn completions_rejects_unsupported_shell() {
        assert!(Cli::try_parse_from(["bluetooth-tracker", "completions", "nushell"]).is_err());
    }
}
