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
mod telemetry;
mod web;

use std::process::ExitCode;
use std::time::Instant;

use clap::{CommandFactory, FromArgMatches};

use crate::cli::{Cli, Command};
use crate::error::CliError;

pub async fn main() -> ExitCode {
    // Parse through ArgMatches rather than `Cli::parse()` so usage reporting
    // can tell which options the user actually typed.
    let matches = Cli::command().get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());

    let decision = telemetry::resolve(&cli);
    if decision == telemetry::Decision::Interrupted {
        return ExitCode::from(telemetry::INTERRUPTED_EXIT_CODE);
    }
    let is_serve = matches!(cli.command, Command::Serve(_));
    if is_serve {
        telemetry::report(&decision, &matches, telemetry::Outcome::Started, None).await;
    }

    let started = Instant::now();
    let result = run(cli).await;

    if !is_serve {
        let outcome = match &result {
            Ok(()) => telemetry::Outcome::Ok,
            Err(error) => telemetry::Outcome::Failed(error),
        };
        telemetry::report(&decision, &matches, outcome, Some(started.elapsed())).await;
    }

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {}", error.display_message());
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Render(arguments) => features::rendering::cli::run(arguments, cli.non_interactive),
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
        Command::Telemetry(arguments) => features::telemetry::cli::run(arguments),
    }
}
