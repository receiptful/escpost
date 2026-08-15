//! Linux-only `printers grant-usb-permissions` terminal command.
//!
//! This is deliberately an adapter-owned capability: it changes host udev
//! configuration, invokes `udevadm`, and presents terminal guidance. It is
//! not an application operation and has no HTTP-facing representation.

use std::fs::{self, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use inquire::Confirm;

use super::GrantUsbPermissionsArgs;
use crate::error::CliError;

/// The sole udev rule file managed by this command.
const RULES_PATH: &str = "/etc/udev/rules.d/70-escpost-usb-printers.rules";

/// The sole rule content used for inspection, writing, and terminal guidance.
const RULE_CONTENT: &str = "\
# Grant locally logged-in users access to USB printer-class devices (escpost).
SUBSYSTEM==\"usb\", ENV{ID_USB_INTERFACES}==\"*:0701*:*\", TAG+=\"uaccess\"
";

static TEMPORARY_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn run(
    _arguments: GrantUsbPermissionsArgs,
    non_interactive: bool,
) -> Result<(), CliError> {
    if !running_as_root() {
        return Err(CliError::GrantUsbPermissionsNeedsRoot {
            guidance: needs_root_guidance(),
        });
    }

    let can_prompt = !non_interactive && io::stdin().is_terminal() && io::stderr().is_terminal();
    if can_prompt {
        print!("{}", describe_change());
    }
    if !should_apply(can_prompt, &mut InquireConfirmPrompter)? {
        println!("Nothing changed.");
        return Ok(());
    }

    match apply_at(Path::new(RULES_PATH), reload_udev)? {
        RuleChange::Created => println!("Wrote {RULES_PATH}"),
        RuleChange::AlreadyCurrent => {
            println!("{RULES_PATH} already grants this access; leaving it unchanged.");
        }
    }
    println!("Replug the USB printer, then run: escpost printers discover");
    print!("{}", undo_commands());
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuleChange {
    Created,
    AlreadyCurrent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuleDecision {
    Create,
    AlreadyCurrent,
    Diverges,
}

fn apply_at(
    path: &Path,
    reload: impl FnOnce() -> Result<(), CliError>,
) -> Result<RuleChange, CliError> {
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
    Ok(change)
}

fn decide_rule(existing: Option<&str>) -> RuleDecision {
    match existing {
        None => RuleDecision::Create,
        Some(existing) if existing == RULE_CONTENT => RuleDecision::AlreadyCurrent,
        Some(_) => RuleDecision::Diverges,
    }
}

fn read_existing_rule(path: &Path) -> Result<Option<String>, CliError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(CliError::ReadUsbRulesFile {
            path: path.to_owned(),
            source,
        }),
    }
}

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
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(&temporary)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|source| CliError::WriteUsbRulesFile {
        path: path.to_owned(),
        source,
    })
}

fn reload_udev() -> Result<(), CliError> {
    run_udevadm(&["control", "--reload"])?;
    run_udevadm(&["trigger", "--subsystem-match=usb"])
}

fn run_udevadm(args: &[&str]) -> Result<(), CliError> {
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

fn describe_change() -> String {
    format!(
        "\
This will write {RULES_PATH}:
{RULE_CONTENT}
Then it will reload udev:
  udevadm control --reload
  udevadm trigger --subsystem-match=usb
"
    )
}

fn needs_root_guidance() -> String {
    format!(
        "Let escpost apply it:
  sudo escpost printers grant-usb-permissions

Or run the commands yourself:
{}",
        manual_commands()
    )
    .trim_end()
    .to_owned()
}

fn manual_commands() -> String {
    format!(
        "  sudo tee {RULES_PATH} <<'EOF'
{RULE_CONTENT}EOF
  sudo udevadm control --reload
  sudo udevadm trigger --subsystem-match=usb
"
    )
}

fn undo_commands() -> String {
    format!(
        "\
Undo this grant later with:
  sudo rm {RULES_PATH}
  sudo udevadm control --reload
  sudo udevadm trigger --subsystem-match=usb
Then unplug and replug the printer to be certain access is fully revoked.
"
    )
}

fn should_apply(can_prompt: bool, prompter: &mut impl ConfirmPrompter) -> Result<bool, CliError> {
    if can_prompt {
        prompter.confirm_grant()
    } else {
        Ok(true)
    }
}

trait ConfirmPrompter {
    fn confirm_grant(&mut self) -> Result<bool, CliError>;
}

struct InquireConfirmPrompter;

impl ConfirmPrompter for InquireConfirmPrompter {
    fn confirm_grant(&mut self) -> Result<bool, CliError> {
        Confirm::new("Write the rule and reload udev?")
            .with_default(true)
            .prompt()
            .map_err(|error| CliError::ConfirmationPrompt(error.to_string()))
    }
}

fn running_as_root() -> bool {
    rustix::process::geteuid().as_raw() == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::printers::test_support::temporary_configuration;
    use std::cell::Cell;

    #[test]
    fn applying_an_absent_rule_creates_it_and_a_matching_rule_is_idempotent() {
        let rule = temporary_configuration("grant-usb", "");

        assert_eq!(
            apply_at(rule.path(), || Ok(())).expect("an absent rule should be created"),
            RuleChange::Created
        );
        assert_eq!(
            fs::read_to_string(rule.path()).expect("the new rule should be readable"),
            RULE_CONTENT
        );
        assert_eq!(
            apply_at(rule.path(), || Ok(())).expect("a matching rule should be retained"),
            RuleChange::AlreadyCurrent
        );
    }

    #[test]
    fn applying_a_divergent_rule_refuses_to_overwrite_it() {
        let rule = temporary_configuration("divergent-usb", "MODE=\"0666\"\n");
        let reloaded = Cell::new(false);

        let error = apply_at(rule.path(), || {
            reloaded.set(true);
            Ok(())
        })
        .expect_err("a hand-edited rule must not be overwritten");

        assert!(matches!(error, CliError::UsbRuleDiverges { .. }));
        assert_eq!(
            fs::read_to_string(rule.path()).expect("the divergent rule should remain intact"),
            "MODE=\"0666\"\n"
        );
        assert!(!reloaded.get(), "a refused rule must not reload udev");
    }

    #[test]
    fn manual_commands_write_the_same_rule_the_command_manages() {
        let commands = manual_commands();
        let body = commands
            .split_once("<<'EOF'\n")
            .expect("the manual command should have a quoted heredoc")
            .1
            .split_once("EOF\n")
            .expect("the manual command should close its heredoc")
            .0;

        assert_eq!(body, RULE_CONTENT);
    }

    #[test]
    fn confirmation_describes_the_rule_and_udev_commands_the_command_will_apply() {
        assert_eq!(
            describe_change(),
            "\
This will write /etc/udev/rules.d/70-escpost-usb-printers.rules:
# Grant locally logged-in users access to USB printer-class devices (escpost).
SUBSYSTEM==\"usb\", ENV{ID_USB_INTERFACES}==\"*:0701*:*\", TAG+=\"uaccess\"

Then it will reload udev:
  udevadm control --reload
  udevadm trigger --subsystem-match=usb
"
        );
    }

    #[test]
    fn undo_guidance_removes_the_managed_rule_and_reloads_udev() {
        assert_eq!(
            undo_commands(),
            "\
Undo this grant later with:
  sudo rm /etc/udev/rules.d/70-escpost-usb-printers.rules
  sudo udevadm control --reload
  sudo udevadm trigger --subsystem-match=usb
Then unplug and replug the printer to be certain access is fully revoked.
"
        );
    }

    #[test]
    fn non_interactive_execution_does_not_prompt() {
        struct PanicsIfAsked;
        impl ConfirmPrompter for PanicsIfAsked {
            fn confirm_grant(&mut self) -> Result<bool, CliError> {
                panic!("non-interactive execution must not prompt")
            }
        }

        assert!(should_apply(false, &mut PanicsIfAsked).expect("the command should proceed"));
    }

    #[test]
    fn interactive_execution_obeys_the_confirmation_answer() {
        struct Answer(bool);
        impl ConfirmPrompter for Answer {
            fn confirm_grant(&mut self) -> Result<bool, CliError> {
                Ok(self.0)
            }
        }

        assert!(should_apply(true, &mut Answer(true)).expect("yes should proceed"));
        assert!(!should_apply(true, &mut Answer(false)).expect("no should stop"));
    }

    #[test]
    fn interactive_execution_propagates_confirmation_errors() {
        struct FailingPrompter;
        impl ConfirmPrompter for FailingPrompter {
            fn confirm_grant(&mut self) -> Result<bool, CliError> {
                Err(CliError::ConfirmationPrompt("interrupted".to_owned()))
            }
        }

        let error = should_apply(true, &mut FailingPrompter)
            .expect_err("a confirmation error must not be treated as a decline");

        assert!(matches!(error, CliError::ConfirmationPrompt(_)));
    }

    #[test]
    fn divergent_rule_error_identifies_the_existing_and_desired_rules() {
        let rule = temporary_configuration("divergent-message", "MODE=\"0666\"\n");
        let error = apply_at(rule.path(), || Ok(())).expect_err("the rule should be refused");
        let message = error.to_string();

        assert!(message.contains(&rule.path().display().to_string()));
        assert!(message.contains("MODE=\"0666\""));
        assert!(message.contains(RULE_CONTENT));
    }
}
