//! `printers grant-usb-permissions` (Linux only): grant the invoking user's
//! active local session access to USB printer-class devices via a udev rule.
//!
//! USB device nodes under `/dev/bus/usb/` are root-owned by default on
//! Linux, so `printers discover` degrades to a per-device permission
//! warning and any command that opens the device directly — `print` and
//! `printers add`'s USB selection — fails outright until a udev rule grants
//! broader access (`printers list` never opens the device, so it is
//! unaffected). Without root, this command only prints the plan (the rule
//! it would write and the commands it would run) so a user can inspect it
//! before running it with `sudo`; with root, it writes the rule and reloads
//! udev so it takes effect without a reboot.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use std::os::unix::fs::OpenOptionsExt;

use crate::cli::GrantUsbPermissionsArgs;
use crate::error::CliError;

/// Where the rule is installed. Numbered in the 70-series so it loads after
/// the distribution's own base rules without racing them, and named for
/// this crate so it is easy to find and remove.
const RULES_PATH: &str = "/etc/udev/rules.d/70-escpost-usb-printers.rules";

/// Exact contents written to `RULES_PATH`. Matches on interface class 07
/// subclass 01 (USB printer class) rather than a specific vendor/product
/// pair, so any USB printer works after one rule, not only ones escpost
/// already has a profile for. `TAG+="uaccess"` (not `MODE="0666"`) scopes
/// access to whichever user has the active local session, the same
/// mechanism systemd-logind already uses for input and audio devices,
/// rather than opening the device to every local user and process.
const RULE_CONTENT: &str = "\
# Grant locally logged-in users access to USB printer-class devices (escpost).
SUBSYSTEM==\"usb\", ENV{ID_USB_INTERFACES}==\"*:0701*:*\", TAG+=\"uaccess\"
";

static TEMPORARY_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn run(_arguments: GrantUsbPermissionsArgs) -> Result<(), CliError> {
    if !running_as_root() {
        print!("{}", plan());
        eprintln!("Run it with: sudo escpost printers grant-usb-permissions");
        return Ok(());
    }

    let path = Path::new(RULES_PATH);
    let existing = read_existing_rule(path)?;
    match decide_rule_write(existing.as_deref(), RULE_CONTENT) {
        RuleDecision::Write => {
            write_rule_atomically(path, RULE_CONTENT)?;
            println!("Wrote {RULES_PATH}");
        }
        RuleDecision::AlreadyCurrent => {
            println!("{RULES_PATH} already grants this access; leaving it unchanged.");
        }
        RuleDecision::Diverges => {
            return Err(CliError::UsbRuleDiverges {
                path: path.to_owned(),
                existing: existing.unwrap_or_default(),
                desired: RULE_CONTENT.to_owned(),
            });
        }
    }

    reload_udev()?;
    println!("Replug the USB printer, then run: escpost printers discover");
    Ok(())
}

/// What `grant-usb-permissions` prints when it is not running as root: the
/// exact rule it would write, and the commands it would run to load it.
/// Factored out of `run` so its formatting is directly assertable in a unit
/// test without capturing stdout.
fn plan() -> String {
    format!(
        "\
Without root, this only shows what `sudo escpost printers grant-usb-permissions` would do.

Write {RULES_PATH}:
{RULE_CONTENT}
Then run:
  udevadm control --reload
  udevadm trigger --subsystem-match=usb
"
    )
}

#[derive(Debug, PartialEq, Eq)]
enum RuleDecision {
    /// No rule exists yet: write it.
    Write,
    /// The existing rule's content already matches exactly: nothing to
    /// write, but still worth reloading udev in case the file changed
    /// through some other means since the daemon last read it.
    AlreadyCurrent,
    /// The existing rule's content differs: refuse rather than silently
    /// overwriting a possibly hand-edited rule.
    Diverges,
}

/// Pure idempotency decision for the on-disk rule, factored out so it is
/// testable without touching `/etc`. `existing` is `None` when the rules
/// file does not exist yet.
fn decide_rule_write(existing: Option<&str>, desired: &str) -> RuleDecision {
    match existing {
        None => RuleDecision::Write,
        Some(existing) if existing == desired => RuleDecision::AlreadyCurrent,
        Some(_) => RuleDecision::Diverges,
    }
}

/// Effective UID via `rustix::process::geteuid`, a safe wrapper the crate
/// already pulls in transitively through `nusb`. The workspace forbids
/// `unsafe_code` outright, which rules out calling `libc::geteuid` (an
/// `unsafe fn`) directly; `rustix` gives the same syscall through a safe
/// API instead.
fn running_as_root() -> bool {
    rustix::process::geteuid().as_raw() == 0
}

/// Read the rules file's current content, if any. `Ok(None)` means the file
/// does not exist yet, which is the common case and not an error; any other
/// read failure (for example, an unreadable file) is.
fn read_existing_rule(path: &Path) -> Result<Option<String>, CliError> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(CliError::ReadUsbRulesFile {
            path: path.to_owned(),
            source,
        }),
    }
}

/// Replace the rules file only after its complete new contents are written,
/// mirroring `configuration::write_atomically`'s temp-file-then-rename
/// idiom (kept beside the destination so the rename stays on one
/// filesystem) rather than reusing that private helper directly. Unlike a
/// user configuration file, this one must be world-readable (0644): it is
/// read by the udev daemon and, conventionally, by anyone auditing
/// `/etc/udev/rules.d`, not only by the user running this command (who is
/// root at this point regardless).
fn write_rule_atomically(path: &Path, content: &str) -> Result<(), CliError> {
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

/// Reload udev's rule database and re-trigger it for already-connected USB
/// devices, so a printer plugged in before the rule existed does not need a
/// reboot or a manual replug to pick it up. `run()`'s caller still prints a
/// "replug" hint afterward because a trigger only re-runs rules against the
/// kernel's existing device state; it does not force USB re-enumeration the
/// way a physical replug does, and `uaccess` in particular is only fully
/// applied once logind re-evaluates the device.
fn reload_udev() -> Result<(), CliError> {
    run_udevadm(&["control", "--reload"])?;
    run_udevadm(&["trigger", "--subsystem-match=usb"])?;
    Ok(())
}

fn run_udevadm(args: &[&'static str]) -> Result<(), CliError> {
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

    #[test]
    fn rule_content_matches_the_agreed_udev_rule_exactly() {
        assert_eq!(
            RULE_CONTENT,
            "# Grant locally logged-in users access to USB printer-class devices (escpost).\n\
             SUBSYSTEM==\"usb\", ENV{ID_USB_INTERFACES}==\"*:0701*:*\", TAG+=\"uaccess\"\n"
        );
    }

    #[test]
    fn plan_names_the_exact_rules_path_and_content_and_the_udevadm_commands() {
        let plan = plan();

        assert!(
            plan.contains(RULES_PATH),
            "plan should name the rule path:\n{plan}"
        );
        assert!(
            plan.contains(RULE_CONTENT),
            "plan should include the full rule content:\n{plan}"
        );
        assert!(
            plan.contains("udevadm control --reload"),
            "plan should show the reload command:\n{plan}"
        );
        assert!(
            plan.contains("udevadm trigger --subsystem-match=usb"),
            "plan should show the trigger command:\n{plan}"
        );
    }

    #[test]
    fn decide_rule_write_writes_when_no_rule_exists_yet() {
        assert_eq!(decide_rule_write(None, RULE_CONTENT), RuleDecision::Write);
    }

    #[test]
    fn decide_rule_write_is_a_no_op_for_identical_existing_content() {
        assert_eq!(
            decide_rule_write(Some(RULE_CONTENT), RULE_CONTENT),
            RuleDecision::AlreadyCurrent
        );
    }

    #[test]
    fn decide_rule_write_refuses_to_overwrite_different_existing_content() {
        assert_eq!(
            decide_rule_write(Some("MODE=\"0666\"\n"), RULE_CONTENT),
            RuleDecision::Diverges
        );
    }

    #[test]
    fn divergent_rule_error_shows_both_the_existing_and_desired_content() {
        let error = CliError::UsbRuleDiverges {
            path: Path::new(RULES_PATH).to_owned(),
            existing: "MODE=\"0666\"\n".to_owned(),
            desired: RULE_CONTENT.to_owned(),
        };

        let message = error.to_string();
        assert!(
            message.contains(RULES_PATH),
            "the error should name the rule path:\n{message}"
        );
        assert!(
            message.contains("MODE=\"0666\""),
            "the error should show the existing content:\n{message}"
        );
        assert!(
            message.contains(RULE_CONTENT),
            "the error should show the desired content:\n{message}"
        );
    }
}
