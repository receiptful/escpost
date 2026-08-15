//! Non-interactive, explicitly approved Linux USB-permission operation.

pub(crate) mod cli;

use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::application;
use crate::error::CliError;

const RULES_PATH: &str = "/etc/udev/rules.d/70-escpost-usb-printers.rules";

const RULE_CONTENT: &str = "\
# Grant locally logged-in users access to USB printer-class devices (escpost).
SUBSYSTEM==\"usb\", ENV{ID_USB_INTERFACES}==\"*:0701*:*\", TAG+=\"uaccess\"
";
static TEMPORARY_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ApprovedAction {
    GrantUsbPrinterAccess,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub(crate) action: ApprovedAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuleChange {
    Created,
    AlreadyCurrent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Response {
    pub(crate) change: RuleChange,
}

pub(crate) fn execute(request: Request) -> application::Result<Response> {
    match request.action {
        ApprovedAction::GrantUsbPrinterAccess => apply_at(Path::new(RULES_PATH), reload_udev),
    }
}

fn apply_at(
    path: &Path,
    reload: impl FnOnce() -> application::Result<()>,
) -> application::Result<Response> {
    let existing = read_existing_rule(path)?;
    let change = match decide_rule(existing.as_deref()) {
        RuleDecision::Create => {
            write_rule_atomically(path, RULE_CONTENT)?;
            RuleChange::Created
        }
        RuleDecision::AlreadyCurrent => RuleChange::AlreadyCurrent,
        RuleDecision::Diverges => {
            return Err(CliError::UsbRuleDiverges {
                path: path.to_owned(),
                existing: existing.unwrap_or_default(),
                desired: RULE_CONTENT.to_owned(),
            });
        }
    };
    reload()?;
    Ok(Response { change })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuleDecision {
    Create,
    AlreadyCurrent,
    Diverges,
}

fn decide_rule(existing: Option<&str>) -> RuleDecision {
    match existing {
        None => RuleDecision::Create,
        Some(existing) if existing == RULE_CONTENT => RuleDecision::AlreadyCurrent,
        Some(_) => RuleDecision::Diverges,
    }
}

fn read_existing_rule(path: &Path) -> application::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(CliError::ReadUsbRulesFile {
            path: path.to_owned(),
            source,
        }),
    }
}

fn write_rule_atomically(path: &Path, content: &str) -> application::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .expect("the rules path has a file name")
        .to_string_lossy();
    let sequence = TEMPORARY_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        sequence
    ));
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(&temporary)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|source| CliError::WriteUsbRulesFile {
        path: path.to_owned(),
        source,
    })
}

fn reload_udev() -> application::Result<()> {
    run_udevadm(&["control", "--reload"])?;
    run_udevadm(&["trigger", "--subsystem-match=usb"])
}

fn run_udevadm(args: &[&'static str]) -> application::Result<()> {
    let command = format!("udevadm {}", args.join(" "));
    let output = Command::new("udevadm")
        .args(args)
        .output()
        .map_err(|source| CliError::RunUdevadm {
            command: command.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(CliError::UdevadmFailed {
            command,
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::printers::test_support::temporary_configuration;

    #[test]
    fn approved_operation_result_distinguishes_created_from_already_current() {
        let rule = temporary_configuration("typed-grant", "");
        let created = apply_at(rule.path(), || Ok(())).expect("the absent rule should be created");
        let current =
            apply_at(rule.path(), || Ok(())).expect("the current rule should be accepted");

        assert_eq!(created.change, RuleChange::Created);
        assert_eq!(current.change, RuleChange::AlreadyCurrent);
        assert_eq!(
            decide_rule(Some(RULE_CONTENT)),
            RuleDecision::AlreadyCurrent
        );
        assert_eq!(
            decide_rule(Some("a locally modified rule\n")),
            RuleDecision::Diverges,
            "a divergent rule remains a refusal rather than being replaced"
        );
    }
}
