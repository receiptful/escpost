//! The stored consent answer and the environment variables that override it.
//!
//! `telemetry.toml` is deliberately a separate file from `printers.toml`:
//! deleting or hand-editing printer configuration must never change the
//! consent answer, and the other way round.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::application;
use crate::configuration;

pub(crate) const TELEMETRY_FILE: &str = "telemetry.toml";
const DO_NOT_TRACK_ENV: &str = "DO_NOT_TRACK";
const ESCPOST_TELEMETRY_ENV: &str = "ESCPOST_TELEMETRY";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StoredDecision {
    pub(crate) enabled: bool,
    pub(crate) installation_id: String,
    pub(crate) decided_at: String,
}

/// Which environment variable switched telemetry off, so `telemetry status`
/// can name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnvironmentOverride {
    DoNotTrack,
    EscpostTelemetry,
}

impl EnvironmentOverride {
    pub(crate) fn variable(self) -> &'static str {
        match self {
            Self::DoNotTrack => DO_NOT_TRACK_ENV,
            Self::EscpostTelemetry => ESCPOST_TELEMETRY_ENV,
        }
    }
}

pub(crate) fn environment_override() -> Option<EnvironmentOverride> {
    let do_not_track = env::var(DO_NOT_TRACK_ENV).ok();
    let escpost_telemetry = env::var(ESCPOST_TELEMETRY_ENV).ok();
    disabled_by(do_not_track.as_deref(), escpost_telemetry.as_deref())
}

/// `DO_NOT_TRACK` follows the consoledonottrack.com convention: set and not
/// `0` means off. `ESCPOST_TELEMETRY` is the tool-specific switch and only
/// its explicit off values disable, so `ESCPOST_TELEMETRY=1` never surprises
/// a user who meant "on".
pub(crate) fn disabled_by(
    do_not_track: Option<&str>,
    escpost_telemetry: Option<&str>,
) -> Option<EnvironmentOverride> {
    if do_not_track.is_some_and(|value| !value.is_empty() && value != "0") {
        return Some(EnvironmentOverride::DoNotTrack);
    }
    if escpost_telemetry
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "0" | "false" | "off"))
    {
        return Some(EnvironmentOverride::EscpostTelemetry);
    }
    None
}

pub(crate) fn file_path() -> application::Result<PathBuf> {
    configuration::directory().map(|directory| directory.join(TELEMETRY_FILE))
}

/// A missing, unreadable, or malformed file all mean "undecided". Telemetry
/// is never allowed to fail a command, so there is no error to return here.
pub(crate) fn load_from(path: &Path) -> Option<StoredDecision> {
    let content = fs::read_to_string(path).ok()?;
    let decision: StoredDecision = toml::from_str(&content).ok()?;
    if decision.installation_id.is_empty() {
        return None;
    }
    Some(decision)
}

/// Write the decision, keeping an existing installation ID so an opt-out and
/// a later opt-in still count as one installation.
pub(crate) fn store_at(path: &Path, enabled: bool) -> io::Result<StoredDecision> {
    let installation_id = load_from(path)
        .map(|existing| existing.installation_id)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let decided_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let decision = StoredDecision {
        enabled,
        installation_id,
        decided_at,
    };
    let content =
        toml::to_string_pretty(&decision).map_err(|error| io::Error::other(error.to_string()))?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    configuration::write_atomically(path, content.as_bytes())?;
    Ok(decision)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn do_not_track_disables_unless_it_is_zero() {
        assert_eq!(
            disabled_by(Some("1"), None),
            Some(EnvironmentOverride::DoNotTrack)
        );
        assert_eq!(
            disabled_by(Some("yes"), None),
            Some(EnvironmentOverride::DoNotTrack)
        );
        assert_eq!(disabled_by(Some("0"), None), None);
        assert_eq!(disabled_by(Some(""), None), None);
        assert_eq!(disabled_by(None, None), None);
    }

    #[test]
    fn escpost_telemetry_disables_only_for_off_values() {
        for value in ["0", "false", "off", "FALSE", "Off"] {
            assert_eq!(
                disabled_by(None, Some(value)),
                Some(EnvironmentOverride::EscpostTelemetry),
                "{value:?} should disable"
            );
        }
        // Review focus 3: any other value keeps the stored decision in force.
        for value in ["1", "true", "on", "", "yes"] {
            assert_eq!(
                disabled_by(None, Some(value)),
                None,
                "{value:?} must not disable"
            );
        }
    }

    #[test]
    fn do_not_track_wins_over_escpost_telemetry() {
        assert_eq!(
            disabled_by(Some("1"), Some("1")),
            Some(EnvironmentOverride::DoNotTrack)
        );
    }

    #[test]
    fn a_missing_file_is_undecided() {
        let directory = temporary_directory("missing");
        assert_eq!(load_from(&directory.join(TELEMETRY_FILE)), None);
    }

    #[test]
    fn storing_a_decision_round_trips_and_mints_one_installation_id() {
        let directory = temporary_directory("round-trip");
        let path = directory.join(TELEMETRY_FILE);

        let stored = store_at(&path, true).expect("the decision should be written");

        assert!(stored.enabled);
        assert_eq!(stored.installation_id.len(), 36, "a hyphenated UUID");
        assert!(stored.decided_at.ends_with('Z'), "RFC 3339 in UTC");
        assert_eq!(load_from(&path), Some(stored));
    }

    #[test]
    fn opting_out_keeps_the_installation_id() {
        // A later opt-in must not look like a brand new installation.
        let directory = temporary_directory("opt-out");
        let path = directory.join(TELEMETRY_FILE);
        let first = store_at(&path, true).expect("opt-in should be written");

        let second = store_at(&path, false).expect("opt-out should be written");

        assert!(!second.enabled);
        assert_eq!(second.installation_id, first.installation_id);
    }

    #[test]
    fn a_garbage_file_is_undecided() {
        let directory = temporary_directory("garbage");
        let path = directory.join(TELEMETRY_FILE);
        std::fs::write(&path, "this is = not [valid").expect("fixture should be written");

        assert_eq!(load_from(&path), None);
    }

    #[test]
    fn a_file_without_an_installation_id_is_undecided() {
        // Review focus 1: a hand-edited file that lost its ID cannot be used,
        // because every event needs a distinct_id.
        let directory = temporary_directory("no-id");
        let path = directory.join(TELEMETRY_FILE);
        std::fs::write(&path, "enabled = true\n").expect("fixture should be written");

        assert_eq!(load_from(&path), None);
        let stored = store_at(&path, true).expect("a fresh decision should be written");
        assert_eq!(stored.installation_id.len(), 36);
    }

    #[test]
    fn storing_under_an_uncreatable_directory_is_an_error_not_a_panic() {
        // Review focus 2: the caller swallows this and keeps running the command.
        let directory = temporary_directory("uncreatable");
        let blocker = directory.join("file");
        std::fs::write(&blocker, "").expect("fixture should be written");

        let result = store_at(&blocker.join("escpost").join(TELEMETRY_FILE), true);

        assert!(result.is_err());
    }

    fn temporary_directory(case: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock should be after the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "escpost-telemetry-consent-{case}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("the temporary directory should be creatable");
        path
    }
}
