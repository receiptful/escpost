//! `printers grant-usb-permissions` (Linux only): grant the invoking user's
//! active local session access to USB printer-class devices via a udev rule.
//!
//! USB device nodes under `/dev/bus/usb/` are root-owned by default on
//! Linux, so `printers discover` degrades to a per-device permission
//! warning and any command that opens the device directly — `print` and
//! `printers add`'s USB selection — fails outright until a udev rule grants
//! broader access (`printers list` never opens the device, so it is
//! unaffected). Without root, this command cannot do what it was asked to
//! do, so it fails (`CliError::GrantUsbPermissionsNeedsRoot`, exit 1)
//! instead of exiting 0 with an informational print; its error message
//! embeds the same two ways to grant the access anyway — rerun this same
//! command with `sudo`, or paste the equivalent bare-metal `tee`/`udevadm`
//! commands for anyone who would rather not run this binary as root at all
//! — so the failure is still actionable. With root and an interactive
//! terminal, it shows the rule and commands it is about to apply, then asks
//! for confirmation (`inquire::Confirm`, default yes) before touching
//! anything; declining leaves the system unchanged. With root and no prompt
//! available (`--non-interactive`, or stdin/stderr not a terminal — the
//! scripted provisioning path), it applies immediately without asking, since
//! the confirmation's own default answer is yes. Applying writes the rule
//! and reloads udev so it takes effect without a reboot.

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
        return Err(CliError::GrantUsbPermissionsNeedsRoot);
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
    print!("{}", undo_commands());
    Ok(())
}

/// The rule and commands the root+interactive confirmation shows
/// immediately before asking `ConfirmPrompter::confirm_grant` — the exact
/// path this command would write, the full rule content, and the udevadm
/// commands it would run afterward, all built from `RULES_PATH`/
/// `RULE_CONTENT` rather than a second, hand-typed copy of them. Phrased in
/// first person ("This will write...", "Then it will reload udev...")
/// because `run` itself performs these steps right after confirmation, so
/// an imperative "Write ... / Then run: udevadm ..." here would misdescribe
/// who is about to act. Unlike `manual_commands` below (the non-root
/// path's bare-metal equivalent, which stays imperative since a human runs
/// those commands themselves), this is prose describing what `run` itself
/// is about to do, not a paste-and-run shell block, so it stays its own
/// function rather than being unified with it.
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

/// The two independent ways to grant the access, embedded in
/// `CliError::GrantUsbPermissionsNeedsRoot`'s message (see `error.rs`) so
/// the without-root failure is still actionable rather than a bare
/// "requires root". `pub(crate)` and re-exported from `printers::mod`
/// specifically so `error.rs` can build that error's `#[error(...)]` text
/// from it, the same way several existing `CliError` variants already call
/// into `crate::configuration::display_path` from their own attributes —
/// this keeps the guidance defined exactly once rather than duplicated
/// between the error type and this module. No trailing newline: the
/// `#[error(...)]` interpolation site and, ultimately, `eprintln!` in
/// `lib.rs` each contribute exactly the newlines needed around it.
pub(crate) fn needs_root_guidance() -> String {
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

/// Bare-metal commands a developer can paste directly into a root shell to
/// apply the exact same rule `grant-usb-permissions` itself would, without
/// installing or trusting this binary to do it. Built from `RULES_PATH`/
/// `RULE_CONTENT`, never a second hand-typed copy of them, so the pasted
/// and the applied rule cannot drift apart — pinned by
/// `manual_commands_heredoc_body_equals_the_rule_constant_exactly` below,
/// and independently re-verified against a real shell (see the fix
/// report). The heredoc body (the rule content) is written flush-left, not
/// indented like the surrounding command lines: `bash` keeps any leading
/// whitespace on heredoc body lines as part of the file it writes, so
/// indenting it here to visually match the command lines around it would
/// silently corrupt the pasted rule. `<<'EOF'` is quoted so nothing in the
/// rule gets shell-expanded before `tee` ever sees it — nothing in
/// `RULE_CONTENT` is expandable today, but quoting costs nothing and stays
/// correct if that ever changes.
fn manual_commands() -> String {
    // Deliberately not opening with the `"\` line-continuation the other
    // functions in this file use to keep their first content line
    // flush-left in the *source* despite Rust's own indentation: that
    // continuation strips leading whitespace from the line right after it,
    // which would silently eat this block's leading two spaces on its
    // first (`sudo tee`) line. Starting the literal's first line inline
    // with the opening quote avoids that stripping entirely.
    format!(
        "  sudo tee {RULES_PATH} <<'EOF'
{RULE_CONTENT}EOF
  sudo udevadm control --reload
  sudo udevadm trigger --subsystem-match=usb
"
    )
}

/// Printed after a root-mode apply that leaves the rule in place — a fresh
/// write or an idempotent rerun that found it already current, both of
/// which mean the grant is now active — so a user who applied this later
/// has a documented way back out, instead of having to reverse-engineer
/// `manual_commands` by hand. Never printed on decline or on the
/// `UsbRuleDiverges` refusal, since neither of those actually grants
/// anything to undo. The `rm` target comes from `RULES_PATH`, never a
/// hand-typed path, so it cannot name the wrong file. The trailing
/// replug caveat is not filler: udev's `uaccess` tag grants access via a
/// logind ACL applied when the device is plugged in, and removing the
/// rule does not retroactively strip that ACL from an already-plugged
/// device — only a fresh plug (which logind re-evaluates against the
/// now-gone rule) actually revokes it.
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
        // itself rejects) is a hard error rather than silently treated as
        // declined, mirroring how `InquireAddPrompter` treats every one of
        // its own prompt failures in `add.rs` — but through this command's
        // own `ConfirmationPrompt` variant, not `add.rs`'s `PrinterPrompt`:
        // that message reads "could not read printer information", which
        // is misleading for a system-change confirmation that has nothing
        // to do with printer information.
        Confirm::new("Write the rule and reload udev?")
            .with_default(true)
            .prompt()
            .map_err(|error| CliError::ConfirmationPrompt(error.to_string()))
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
    fn describe_change_matches_the_exact_first_person_format() {
        // `run` performs these steps itself right after confirmation, so
        // this must read as narration ("This will write...", "Then it
        // will reload udev...") rather than instructions to the user —
        // unlike `manual_commands`/`needs_root_guidance`, which stay
        // imperative since a human runs those. A full literal comparison
        // pins the exact wording, not just that the path/content/commands
        // appear somewhere in it.
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
    fn needs_root_guidance_matches_the_exact_two_ways_format() {
        // A full literal comparison, not just substring checks: this text
        // is embedded verbatim in `CliError::GrantUsbPermissionsNeedsRoot`
        // (see `error.rs`'s own exact-match test for the complete rendered
        // error), so its shape (the two options, the blank lines between
        // them, the indentation, the heredoc marker, no trailing newline)
        // matters as much as its content.
        assert_eq!(
            needs_root_guidance(),
            "\
Let escpost apply it:
  sudo escpost printers grant-usb-permissions

Or run the commands yourself:
  sudo tee /etc/udev/rules.d/70-escpost-usb-printers.rules <<'EOF'
# Grant locally logged-in users access to USB printer-class devices (escpost).
SUBSYSTEM==\"usb\", ENV{ID_USB_INTERFACES}==\"*:0701*:*\", TAG+=\"uaccess\"
EOF
  sudo udevadm control --reload
  sudo udevadm trigger --subsystem-match=usb"
        );
    }

    #[test]
    fn manual_commands_embeds_the_rule_constant_verbatim_instead_of_duplicating_it() {
        let commands = manual_commands();

        assert!(
            commands.contains(RULE_CONTENT),
            "the heredoc body should be RULE_CONTENT verbatim, not a second hand-typed copy:\n{commands}"
        );
    }

    /// Pulls the heredoc body out of `manual_commands()`'s tee block: the
    /// bytes `bash` actually writes to disk when a developer pastes the
    /// block verbatim. A small, independent parser rather than eyeballing
    /// the format string, so the test below really checks what the shell
    /// would see.
    fn extract_heredoc_body(commands: &str) -> &str {
        let after_marker = commands
            .split_once("<<'EOF'")
            .expect("the block should open a quoted-EOF heredoc")
            .1;
        let body_start = after_marker
            .find('\n')
            .expect("a newline should follow the heredoc marker")
            + 1;
        let body = &after_marker[body_start..];
        let terminator = body
            .find("\nEOF\n")
            .expect("the heredoc should close with a flush-left EOF line");
        &body[..=terminator]
    }

    #[test]
    fn manual_commands_heredoc_body_equals_the_rule_constant_exactly() {
        // The requirement this pins: pasting the printed block into a
        // shell must reproduce RULE_CONTENT byte-for-byte, not merely
        // "contain" it or resemble it. See the fix report for the same
        // check re-run against a real shell, independent of this parser.
        assert_eq!(extract_heredoc_body(&manual_commands()), RULE_CONTENT);
    }

    #[test]
    fn undo_commands_matches_the_exact_format() {
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
    fn undo_commands_removes_the_exact_rules_path() {
        assert!(
            undo_commands().contains(&format!("sudo rm {RULES_PATH}")),
            "the undo block should remove RULES_PATH exactly, not a hand-typed path"
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
                Err(CliError::ConfirmationPrompt("interrupted".to_owned()))
            }
        }

        let error = should_apply(true, &mut FailingPrompter)
            .expect_err("a prompt failure must propagate, not be swallowed as declined");

        assert!(matches!(error, CliError::ConfirmationPrompt(_)));
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
