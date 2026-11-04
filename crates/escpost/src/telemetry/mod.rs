//! Anonymous usage reporting: consent storage, event construction, delivery.
//!
//! The only caller is the root dispatcher in `lib.rs`. Feature modules never
//! touch this module, so what is reported is decided in one place.

pub(crate) mod consent;
pub(crate) mod event;
pub(crate) mod sink;

use std::env;
use std::io::IsTerminal;
use std::path::Path;
use std::time::Duration;

use clap::ArgMatches;
use inquire::Confirm;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::cli::{Cli, Command};
use crate::error::CliError;

const DEBUG_ENV: &str = "ESCPOST_TELEMETRY_DEBUG";
pub(crate) const PRIVACY_URL: &str = "https://escpost.dev/dataprivacy";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decision {
    Disabled,
    Undecided,
    Enabled {
        installation_id: String,
        /// Consent was given during this very invocation.
        first_run: bool,
    },
}

pub(crate) enum Outcome<'a> {
    Ok,
    /// `serve` may never return, so it reports when it starts.
    Started,
    Failed(&'a CliError),
}

/// Decide, before the command runs, whether this invocation reports.
///
/// Order: environment override, stored answer, prompt if possible, otherwise
/// undecided. Every failure path ends in `Undecided`, because reporting must
/// never stop a command from running.
pub(crate) fn resolve(cli: &Cli) -> Decision {
    if consent::environment_override().is_some() {
        return Decision::Disabled;
    }
    let Ok(path) = consent::file_path() else {
        return Decision::Undecided;
    };
    if let Some(stored) = consent::load_from(&path) {
        return if stored.enabled {
            Decision::Enabled {
                installation_id: stored.installation_id,
                first_run: false,
            }
        } else {
            Decision::Disabled
        };
    }
    if !can_prompt(cli) {
        return Decision::Undecided;
    }
    prompt_and_store(&path)
}

/// Send the invocation's event when the decision allows it. Failures are
/// swallowed; `ESCPOST_TELEMETRY_DEBUG=1` reports them on stderr.
pub(crate) async fn report(
    decision: &Decision,
    matches: &ArgMatches,
    outcome: Outcome<'_>,
    duration: Option<Duration>,
) {
    let Decision::Enabled {
        installation_id,
        first_run,
    } = decision
    else {
        return;
    };
    let outcome = match outcome {
        Outcome::Ok => "ok".to_owned(),
        Outcome::Started => "started".to_owned(),
        Outcome::Failed(error) => error.telemetry_kind(),
    };
    let timestamp = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default();
    let event = event::build(
        matches,
        installation_id,
        &outcome,
        duration,
        *first_run,
        timestamp,
    );
    let endpoint = sink::endpoint();
    let result = sink::send(&endpoint, &event).await;
    if debug_enabled() {
        match result {
            Ok(()) => eprintln!("telemetry: sent {} to {endpoint}", event::EVENT_NAME),
            Err(error) => eprintln!("telemetry: not sent to {endpoint}: {error}"),
        }
    }
}

fn can_prompt(cli: &Cli) -> bool {
    !cli.non_interactive
        && !reads_receipt_from_stdin(&cli.command)
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
        && std::io::stderr().is_terminal()
}

/// `render -` and `print -` take the receipt from stdin; a prompt would
/// swallow receipt bytes as its answer.
fn reads_receipt_from_stdin(command: &Command) -> bool {
    match command {
        Command::Render(arguments) => arguments.source == Path::new("-"),
        Command::Print(arguments) => arguments.source == Path::new("-"),
        _ => false,
    }
}

fn prompt_and_store(path: &Path) -> Decision {
    eprintln!(
        "\
ESCPost can share anonymous usage data to help improve the tool.

  Sent:       command name, flag names, success or error kind, duration,
              CLI version, operating system, CPU architecture
  Never sent: receipt data, file names, printer names, addresses, serial
              numbers, or anything that identifies you or your machine

  Change later:  escpost telemetry off
  Details:       {PRIVACY_URL}
"
    );
    let answer = Confirm::new("Share anonymous usage data?")
        .with_default(true)
        .prompt();
    let Ok(enabled) = answer else {
        // A closed terminal or Ctrl+C is not a "no"; ask again next time.
        return Decision::Undecided;
    };
    match consent::store_at(path, enabled) {
        Ok(stored) => {
            eprintln!(
                "Usage data {}. Saved to {}.\n",
                if enabled { "enabled" } else { "disabled" },
                path.display()
            );
            if enabled {
                Decision::Enabled {
                    installation_id: stored.installation_id,
                    first_run: true,
                }
            } else {
                Decision::Disabled
            }
        }
        Err(error) => {
            eprintln!(
                "warning: could not save the usage data choice to {}: {error}\n",
                path.display()
            );
            Decision::Undecided
        }
    }
}

fn debug_enabled() -> bool {
    env::var(DEBUG_ENV).is_ok_and(|value| value == "1")
}
