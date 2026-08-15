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
//! before running it with `sudo`. With root and an interactive terminal, it
//! shows the same rule and commands, then asks for confirmation
//! (`inquire::Confirm`, default yes) before touching anything; declining
//! leaves the system unchanged. With root and no prompt available
//! (`--non-interactive`, or stdin/stderr not a terminal — the scripted
//! provisioning path), it applies immediately without asking, since the
//! confirmation's own default answer is yes. Applying writes the rule and
//! reloads udev so it takes effect without a reboot.

use std::fs::OpenOptions;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use std::os::unix::fs::OpenOptionsExt;

use inquire::Confirm;

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

pub(super) fn run(
    _arguments: GrantUsbPermissionsArgs,
    non_interactive: bool,
) -> Result<(), CliError> {
    if !running_as_root() {
        print!("{}", plan());
        eprintln!("Run it with: sudo escpost printers grant-usb-permissions");
        return Ok(());
    }

    let can_prompt = !non_interactive && io::stdin().is_terminal() && io::stderr().is_terminal();
    if can_prompt {
        print!("{}", describe_change());
    }
    if !should_apply(can_prompt, &mut InquireConfirmPrompter)? {
        println!("Nothing changed.");
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

/// The rule and commands `grant-usb-permissions` is about to apply: the
/// exact path it would write, the full rule content, and the udevadm
/// commands it would run afterward. Shared verbatim by both the non-root
/// informational `plan` (which wraps this with why it's being shown and,
/// separately, the sudo hint) and the root+interactive confirmation prompt
/// (which shows this immediately before asking for confirmation), so the
/// rule path/content/commands are described exactly once in the source
/// rather than duplicated between the two call sites.
fn describe_change() -> String {
    format!(
        "\
Write {RULES_PATH}:
{RULE_CONTENT}
Then run:
  udevadm control --reload
  udevadm trigger --subsystem-match=usb
"
    )
}

/// What `grant-usb-permissions` prints when it is not running as root: the
/// same rule and commands `describe_change` shows, wrapped with why they're
/// being shown instead of applied. Factored out of `run` so its formatting
/// is directly assertable in a unit test without capturing stdout.
fn plan() -> String {
    format!(
        "Without root, this only shows what `sudo escpost printers grant-usb-permissions` would do.\n\n{}",
        describe_change()
    )
}

/// Whether to go ahead and write the rule, given whether a confirmation
/// prompt is even possible and, if so, the user's answer. The pure decision
/// seam behind the root+interactive confirmation: `can_prompt: false` (the
/// `--non-interactive` or no-tty scripted-provisioning path) always
/// proceeds without ever touching `prompter`, matching the confirmation's
/// own default-yes answer; `can_prompt: true` defers entirely to
/// `prompter.confirm_grant()`. Testable without root or a real terminal by
/// swapping in a `ConfirmPrompter` double — the actual rule write and udev
/// reload that follow a `true` result stay root-only and untested here.
fn should_apply(can_prompt: bool, prompter: &mut impl ConfirmPrompter) -> Result<bool, CliError> {
    if !can_prompt {
        return Ok(true);
    }
    prompter.confirm_grant()
}

/// A yes/no confirmation before `grant-usb-permissions` changes the system.
/// Deliberately its own minimal trait rather than reusing `add`'s
/// `AddPrompter`: this command needs exactly one answer, not a family of
/// prompts, and the two commands share no prompting state.
trait ConfirmPrompter {
    fn confirm_grant(&mut self) -> Result<bool, CliError>;
}

struct InquireConfirmPrompter;

impl ConfirmPrompter for InquireConfirmPrompter {
    fn confirm_grant(&mut self) -> Result<bool, CliError> {
        // Any prompt failure (Esc, Ctrl-C, a non-interactive stream inquire
        // itself rejects) maps to `PrinterPrompt`, the same catch-all
        // `InquireAddPrompter` uses for every one of its own prompts in
        // `add.rs` — there is no "treat cancellation as declined" special
        // case there, so this stays consistent rather than inventing one
        // here for the one place `grant-usb-permissions` prompts at all.
        Confirm::new("Write the rule and reload udev?")
            .with_default(true)
            .prompt()
            .map_err(|error| CliError::PrinterPrompt(error.to_string()))
    }
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
    fn plan_wraps_describe_change_verbatim_instead_of_duplicating_it() {
        // The root+prompt path shows `describe_change()` on its own; this
        // pins that the non-root `plan()` is that exact same text with a
        // prefix, not a second, independently written copy of the rule
        // path/content/commands that could drift out of sync with it.
        let plan = plan();

        assert!(
            plan.ends_with(&describe_change()),
            "plan should end with describe_change()'s exact text:\n{plan}"
        );
    }

    /// A `ConfirmPrompter` double that panics if ever called, for asserting
    /// the `can_prompt: false` path never prompts at all.
    struct PanicsIfAskedPrompter;

    impl ConfirmPrompter for PanicsIfAskedPrompter {
        fn confirm_grant(&mut self) -> Result<bool, CliError> {
            panic!("should_apply must not prompt when a prompt is not possible");
        }
    }

    /// A `ConfirmPrompter` double returning a fixed answer.
    struct FixedConfirmPrompter(bool);

    impl ConfirmPrompter for FixedConfirmPrompter {
        fn confirm_grant(&mut self) -> Result<bool, CliError> {
            Ok(self.0)
        }
    }

    #[test]
    fn should_apply_proceeds_without_prompting_when_a_prompt_is_not_possible() {
        assert!(
            should_apply(false, &mut PanicsIfAskedPrompter)
                .expect("no-prompt path should not error")
        );
    }

    #[test]
    fn should_apply_proceeds_when_the_prompt_is_confirmed() {
        assert!(
            should_apply(true, &mut FixedConfirmPrompter(true))
                .expect("a confirmed prompt should not error")
        );
    }

    #[test]
    fn should_apply_does_not_proceed_when_the_prompt_is_declined() {
        assert!(
            !should_apply(true, &mut FixedConfirmPrompter(false))
                .expect("a declined prompt should not error")
        );
    }

    #[test]
    fn should_apply_propagates_a_prompt_error() {
        struct FailingPrompter;
        impl ConfirmPrompter for FailingPrompter {
            fn confirm_grant(&mut self) -> Result<bool, CliError> {
                Err(CliError::PrinterPrompt("interrupted".to_owned()))
            }
        }

        let error = should_apply(true, &mut FailingPrompter)
            .expect_err("a prompt failure must propagate, not be swallowed as declined");

        assert!(matches!(error, CliError::PrinterPrompt(_)));
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
