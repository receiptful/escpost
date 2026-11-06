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

use clap::{ArgMatches, Args};
use inquire::{Confirm, InquireError};
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
    /// The user hit Ctrl+C or Escape at the prompt. The command must not run.
    Interrupted,
}

/// Exit status for an interrupted prompt: 128 + SIGINT, the shell convention.
pub(crate) const INTERRUPTED_EXIT_CODE: u8 = 130;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryMode {
    /// Hand the body to a detached child and return at once.
    Detached,
    /// Post from this process, inside the send budget.
    Inline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptAnswer {
    Yes,
    No,
    /// Ctrl+C or Escape: the user wants out, not a default.
    Interrupted,
    /// No terminal or a broken one: ask again next time.
    Unavailable,
}

/// Arguments of the hidden `__send-telemetry` delivery helper.
#[derive(Debug, Args)]
pub(crate) struct SendTelemetryArgs {
    /// The serialised event body to post.
    pub(crate) event: String,
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

/// Report the invocation's event when the decision allows it, normally by
/// handing it to a detached helper process so the command is not delayed.
/// Failures are swallowed. With `ESCPOST_TELEMETRY_DEBUG=1` the send runs in
/// this process instead and its result is printed on stderr, because a
/// detached child has no terminal to report to.
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
    let Ok(body) = serde_json::to_string(&event) else {
        return;
    };
    let debug = debug_enabled();
    match delivery_mode(std::process::id(), debug) {
        DeliveryMode::Inline => {
            let endpoint = sink::endpoint();
            let result = sink::send(&endpoint, &body).await;
            if debug {
                match result {
                    Ok(()) => eprintln!("telemetry: sent {} to {endpoint}", event::EVENT_NAME),
                    Err(error) => eprintln!("telemetry: not sent to {endpoint}: {error}"),
                }
            }
        }
        DeliveryMode::Detached => {
            // Nothing to tell the user if the spawn fails: the event is lost.
            let _ = sink::spawn_detached(&body);
        }
    }
}

/// A detached child only helps when it can outlive the command. As PID 1 of
/// a container the command's exit kills the whole pid namespace, child
/// included, so there the send runs inline and pays the network round trip.
/// Debug mode sends inline too, because a detached child has no terminal to
/// report the result on.
fn delivery_mode(pid: u32, debug: bool) -> DeliveryMode {
    if debug || pid == 1 {
        DeliveryMode::Inline
    } else {
        DeliveryMode::Detached
    }
}

/// The body of the hidden `__send-telemetry` helper: post and exit quietly.
pub(crate) async fn deliver(body: &str) {
    let _ = sink::send(&sink::endpoint(), body).await;
}

fn can_prompt(cli: &Cli) -> bool {
    !cli.non_interactive
        && !reads_receipt_from_stdin(&cli.command)
        && !matches!(cli.command, Command::Telemetry(_))
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
ESCPost is free and open source. Help us make it better: share anonymous
usage data so we know which commands people use and where they fail.

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
    let enabled = match classify_prompt(answer) {
        PromptAnswer::Yes => true,
        PromptAnswer::No => false,
        PromptAnswer::Interrupted => return Decision::Interrupted,
        // A closed terminal is not a "no"; ask again next time.
        PromptAnswer::Unavailable => return Decision::Undecided,
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

/// Every other prompt in the crate aborts the command on an interrupt; the
/// consent prompt must not be the one place where Ctrl+C runs it anyway.
fn classify_prompt(answer: Result<bool, InquireError>) -> PromptAnswer {
    match answer {
        Ok(true) => PromptAnswer::Yes,
        Ok(false) => PromptAnswer::No,
        Err(InquireError::OperationInterrupted | InquireError::OperationCanceled) => {
            PromptAnswer::Interrupted
        }
        Err(_) => PromptAnswer::Unavailable,
    }
}

fn debug_enabled() -> bool {
    env::var(DEBUG_ENV).is_ok_and(|value| value == "1")
}

#[cfg(test)]
mod tests {
    use inquire::InquireError;

    use super::*;

    #[test]
    fn ctrl_c_and_escape_at_the_prompt_abort_instead_of_running_the_command() {
        // Every other prompt in the crate aborts on an interrupt. A user who
        // hits Ctrl+C on the first run of `escpost print` must not get a
        // printed receipt as a side effect of declining to answer.
        assert_eq!(
            classify_prompt(Err(InquireError::OperationInterrupted)),
            PromptAnswer::Interrupted
        );
        assert_eq!(
            classify_prompt(Err(InquireError::OperationCanceled)),
            PromptAnswer::Interrupted
        );
    }

    #[test]
    fn a_prompt_that_cannot_run_leaves_the_question_open() {
        assert_eq!(
            classify_prompt(Err(InquireError::NotTTY)),
            PromptAnswer::Unavailable
        );
        assert_eq!(
            classify_prompt(Err(InquireError::IO(std::io::Error::other("gone")))),
            PromptAnswer::Unavailable
        );
    }

    #[test]
    fn pid_one_sends_inline_because_a_detached_child_would_die_with_it() {
        // In a container the command is often PID 1. When it exits, the
        // kernel kills every process left in the pid namespace, including a
        // detached delivery child mid-request, so the event never arrives.
        assert_eq!(delivery_mode(1, false), DeliveryMode::Inline);
        assert_eq!(delivery_mode(4242, false), DeliveryMode::Detached);
    }

    #[test]
    fn debug_mode_sends_inline_whatever_the_pid() {
        assert_eq!(delivery_mode(4242, true), DeliveryMode::Inline);
        assert_eq!(delivery_mode(1, true), DeliveryMode::Inline);
    }

    #[test]
    fn answers_map_to_yes_and_no() {
        assert_eq!(classify_prompt(Ok(true)), PromptAnswer::Yes);
        assert_eq!(classify_prompt(Ok(false)), PromptAnswer::No);
    }
}
