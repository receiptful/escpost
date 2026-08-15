//! Native ESCPost developer command-line interface.

mod cli;
mod configuration;
mod discovery;
mod error;
mod net;
mod output;
mod print;
mod printers;
mod profiles;
pub mod profiles_cmd;
mod render;
mod serve;
mod source;
mod watch;
mod web;

use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, Command};
use crate::error::CliError;

pub async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            // Fatal USB-open permission errors (`print`, `printers add`'s
            // USB selection) get the same actionable hint discover's
            // tolerant warnings already carry, so a user hitting the
            // problem the hard way is not left to find `grant-usb-permissions` in the
            // docs.
            #[cfg(target_os = "linux")]
            if error.is_permission_denied_usb_open() {
                eprintln!("Fix USB permissions with: sudo escpost printers grant-usb-permissions");
            }
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Render(arguments) => render::run(arguments, cli.non_interactive).await,
        Command::Print(arguments) => print::run(arguments, cli.non_interactive).await,
        Command::Serve(arguments) => serve::run(arguments, cli.non_interactive).await,
        Command::Printers(arguments) => printers::run(arguments, cli.non_interactive).await,
        Command::Profiles(arguments) => profiles_cmd::run(arguments, cli.non_interactive),
    }
}
