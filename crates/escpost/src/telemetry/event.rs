//! Build the one event an invocation reports, from facts the dispatcher
//! already holds: the parsed command line, the outcome, and the wall time.
//!
//! Everything here is pure so the exact wire shape is unit-tested. Values of
//! options are never read; only the names of options the user typed.

use std::collections::BTreeSet;
use std::time::Duration;

use clap::parser::ValueSource;
use clap::{ArgMatches, Command, CommandFactory};
use serde::Serialize;

use crate::cli::Cli;

/// PostHog project token. It is a write-only key that is already public in
/// the receiptful.io website, so compiling it in leaks nothing.
pub(crate) const PROJECT_TOKEN: &str = "phc_xhG7Ge6UvYuUeycLJknKgHrQRh9Vaxv3LHZKutToBKJE";
pub(crate) const EVENT_NAME: &str = "escpost_cli_command";
/// Separates CLI events from receiptful.io web events in the shared project.
const SOURCE: &str = "escpost-cli";

#[derive(Debug, Serialize)]
pub(crate) struct Event {
    api_key: &'static str,
    event: &'static str,
    distinct_id: String,
    timestamp: String,
    properties: Properties,
}

#[derive(Debug, Serialize)]
struct Properties {
    /// `false` makes this an anonymous event: no person profile is created
    /// and PostHog bills it at the anonymous rate.
    #[serde(rename = "$process_person_profile")]
    process_person_profile: bool,
    source: &'static str,
    command: String,
    flags: Vec<String>,
    outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    version: &'static str,
    os: &'static str,
    arch: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    first_run: Option<bool>,
}

pub(crate) fn build(
    matches: &ArgMatches,
    installation_id: &str,
    outcome: &str,
    duration: Option<Duration>,
    first_run: bool,
    timestamp: String,
) -> Event {
    Event {
        api_key: PROJECT_TOKEN,
        event: EVENT_NAME,
        distinct_id: installation_id.to_owned(),
        timestamp,
        properties: Properties {
            process_person_profile: false,
            source: SOURCE,
            command: command_path(matches),
            flags: flag_names(matches),
            outcome: outcome.to_owned(),
            duration_ms: duration
                .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX)),
            version: env!("CARGO_PKG_VERSION"),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            first_run: first_run.then_some(true),
        },
    }
}

/// `printers add`, `profiles list`, `render`: the subcommand chain without
/// the binary name.
pub(crate) fn command_path(matches: &ArgMatches) -> String {
    let mut path = Vec::new();
    let mut current = matches;
    while let Some((name, nested)) = current.subcommand() {
        path.push(name);
        current = nested;
    }
    path.join(" ")
}

/// Long option names the user typed, at every subcommand level. Positional
/// arguments and defaulted options are left out; a global option that clap
/// copies into each level is listed once.
pub(crate) fn flag_names(matches: &ArgMatches) -> Vec<String> {
    let mut names = BTreeSet::new();
    collect_flag_names(&Cli::command(), matches, &mut names);
    names.into_iter().collect()
}

fn collect_flag_names(command: &Command, matches: &ArgMatches, names: &mut BTreeSet<String>) {
    for id in matches.ids() {
        if matches.value_source(id.as_str()) != Some(ValueSource::CommandLine) {
            continue;
        }
        let Some(argument) = command
            .get_arguments()
            .find(|argument| argument.get_id() == id)
        else {
            continue;
        };
        if let Some(long) = argument.get_long() {
            names.insert(long.to_owned());
        }
    }
    if let Some((name, nested)) = matches.subcommand()
        && let Some(nested_command) = command.find_subcommand(name)
    {
        collect_flag_names(nested_command, nested, names);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use clap::CommandFactory;

    use super::*;
    use crate::cli::Cli;

    fn matches_for(arguments: &[&str]) -> clap::ArgMatches {
        Cli::command()
            .try_get_matches_from(arguments)
            .expect("the fixture command line should parse")
    }

    #[test]
    fn the_command_path_joins_nested_subcommands_with_spaces() {
        assert_eq!(
            command_path(&matches_for(&["escpost", "render", "job.hex"])),
            "render"
        );
        assert_eq!(
            command_path(&matches_for(&[
                "escpost",
                "printers",
                "add",
                "kitchen",
                "--transport",
                "network",
                "--host",
                "10.0.0.9"
            ])),
            "printers add"
        );
    }

    #[test]
    fn flag_names_are_long_names_only_sorted_and_without_values() {
        let matches = matches_for(&[
            "escpost",
            "render",
            "job.hex",
            "--profile",
            "TM-T88V",
            "-o",
            "out.png",
            "--non-interactive",
        ]);

        let flags = flag_names(&matches);

        assert_eq!(flags, ["non-interactive", "output", "profile"]);
    }

    #[test]
    fn positional_arguments_and_defaulted_options_are_not_flags() {
        // `job.hex` is positional; `--format` and `--scale` have defaults the
        // user did not type. Neither may leak into the event.
        let matches = matches_for(&["escpost", "render", "job.hex"]);

        assert!(flag_names(&matches).is_empty());
    }

    #[test]
    fn repeated_and_propagated_flags_appear_once() {
        // Review focus 4: `--subnet` repeats, and clap copies the global
        // `--non-interactive` into every subcommand level.
        let matches = matches_for(&[
            "escpost",
            "--non-interactive",
            "printers",
            "discover",
            "--subnet",
            "10.0.0.0/24",
            "--subnet",
            "10.0.1.0/24",
        ]);

        let flags = flag_names(&matches);

        assert_eq!(flags, ["non-interactive", "subnet"]);
    }

    #[test]
    fn the_event_carries_exactly_the_documented_properties() {
        let matches = matches_for(&["escpost", "profiles", "list", "--search", "t88"]);

        let event = build(
            &matches,
            "11111111-2222-4333-8444-555555555555",
            "ok",
            Some(Duration::from_millis(412)),
            false,
            "2026-11-04T16:02:11Z".to_owned(),
        );
        let json = serde_json::to_value(&event).expect("the event should serialize");

        assert_eq!(json["api_key"], PROJECT_TOKEN);
        assert_eq!(json["event"], EVENT_NAME);
        assert_eq!(json["distinct_id"], "11111111-2222-4333-8444-555555555555");
        assert_eq!(json["timestamp"], "2026-11-04T16:02:11Z");
        let properties = json["properties"]
            .as_object()
            .expect("properties should be an object");
        let mut keys: Vec<&str> = properties.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "$process_person_profile",
                "arch",
                "command",
                "duration_ms",
                "flags",
                "os",
                "outcome",
                "source",
                "version",
            ]
        );
        assert_eq!(properties["$process_person_profile"], false);
        assert_eq!(properties["source"], "escpost-cli");
        assert_eq!(properties["command"], "profiles list");
        assert_eq!(properties["flags"], serde_json::json!(["search"]));
        assert_eq!(properties["outcome"], "ok");
        assert_eq!(properties["duration_ms"], 412);
        assert_eq!(properties["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(properties["os"], std::env::consts::OS);
        assert_eq!(properties["arch"], std::env::consts::ARCH);
        // The search text itself must never appear anywhere in the body.
        assert!(!json.to_string().contains("t88"));
    }

    #[test]
    fn first_run_is_present_only_when_true_and_duration_only_when_known() {
        let matches = matches_for(&["escpost", "serve", "--listen", "9100"]);

        let event = build(&matches, "id", "started", None, true, "t".to_owned());
        let json = serde_json::to_value(&event).expect("the event should serialize");

        assert_eq!(json["properties"]["first_run"], true);
        assert!(json["properties"].get("duration_ms").is_none());
    }

    #[test]
    fn error_kinds_are_snake_case_variant_names() {
        use crate::application::ApplicationError;
        use crate::error::CliError;

        assert_eq!(CliError::MissingProfile.telemetry_kind(), "missing_profile");
        assert_eq!(
            CliError::NoDiscoveredPrinters(9100).telemetry_kind(),
            "no_discovered_printers"
        );
        assert_eq!(
            CliError::Application(ApplicationError::UnknownProfile("x".to_owned()))
                .telemetry_kind(),
            "application_unknown_profile"
        );
    }
}
