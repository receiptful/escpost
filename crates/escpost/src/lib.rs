//! Native ESCPost developer command-line interface.

mod application;
mod cli;
mod configuration;
mod discovery;
mod error;
mod features;
mod net;
mod output;
mod profiles;
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
            eprintln!("error: {}", error.display_message());
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Render(arguments) => {
            features::rendering::cli::run(arguments, cli.non_interactive).await
        }
        Command::Print(arguments) => {
            features::printing::cli::run(arguments, cli.non_interactive).await
        }
        Command::Serve(arguments) => {
            features::capture::cli::run(arguments, cli.non_interactive).await
        }
        Command::Printers(arguments) => {
            features::printers::cli::run(arguments, cli.non_interactive).await
        }
        Command::Profiles(arguments) => {
            features::profiles::cli::run(arguments, cli.non_interactive)
        }
    }
}
