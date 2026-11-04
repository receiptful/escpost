use std::io::{self, Write};

use clap::{Args, Subcommand};

use crate::error::CliError;
use crate::telemetry::PRIVACY_URL;
use crate::telemetry::consent::{self, EnvironmentOverride};

#[derive(Debug, Args)]
#[command(
    about = "Show or change whether anonymous usage data is shared",
    long_about = "Show or change whether anonymous usage data is shared.\n\n\
        ESCPost asks once on the first interactive run. The answer and a random \
        installation ID live in telemetry.toml in the configuration directory. \
        DO_NOT_TRACK=1 or ESCPOST_TELEMETRY=0 switch sharing off without \
        changing the stored answer.\n\nDetails: https://escpost.dev/dataprivacy"
)]
pub(crate) struct TelemetryArgs {
    #[command(subcommand)]
    pub(crate) command: TelemetryCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub(crate) enum TelemetryCommand {
    /// Share anonymous usage data.
    On,
    /// Stop sharing anonymous usage data.
    Off,
    /// Show the effective state, the installation ID, and the settings file.
    Status,
}

pub(crate) fn run(arguments: TelemetryArgs) -> Result<(), CliError> {
    let path = consent::file_path()?;
    let mut stdout = io::stdout().lock();
    match arguments.command {
        TelemetryCommand::On | TelemetryCommand::Off => {
            let enabled = arguments.command == TelemetryCommand::On;
            let stored = consent::store_at(&path, enabled).map_err(|source| {
                CliError::WriteTelemetrySettings {
                    path: path.clone(),
                    source,
                }
            })?;
            writeln!(
                stdout,
                "Usage data {}. Saved to {}.",
                if stored.enabled {
                    "enabled"
                } else {
                    "disabled"
                },
                path.display()
            )
            .map_err(CliError::WriteHumanOutput)?;
            if let Some(environment) = consent::environment_override() {
                writeln!(
                    stdout,
                    "Note: {} is set, so nothing is shared while it stays set.",
                    environment.variable()
                )
                .map_err(CliError::WriteHumanOutput)?;
            }
        }
        TelemetryCommand::Status => {
            let stored = consent::load_from(&path);
            let state = match (consent::environment_override(), &stored) {
                (Some(EnvironmentOverride::DoNotTrack), _) => "disabled by DO_NOT_TRACK",
                (Some(EnvironmentOverride::EscpostTelemetry), _) => "disabled by ESCPOST_TELEMETRY",
                (None, Some(decision)) if decision.enabled => "enabled",
                (None, Some(_)) => "disabled",
                (None, None) => "undecided",
            };
            writeln!(stdout, "Usage data: {state}").map_err(CliError::WriteHumanOutput)?;
            if let Some(decision) = &stored {
                writeln!(stdout, "Installation ID: {}", decision.installation_id)
                    .map_err(CliError::WriteHumanOutput)?;
            }
            writeln!(stdout, "File: {}", path.display()).map_err(CliError::WriteHumanOutput)?;
            writeln!(stdout, "Details: {PRIVACY_URL}").map_err(CliError::WriteHumanOutput)?;
        }
    }
    Ok(())
}
