use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PAYLOAD: &[u8] = b"\x1b@Receipt\x00\xff\n\n";
const DEADLINE: Duration = Duration::from_secs(10);

#[test]
fn sole_printer_is_used_for_files_and_pipes_with_or_without_non_interactive() {
    for stdin_source in [false, true] {
        for non_interactive in [false, true] {
            let fixture = Fixture::new(false);
            let mut command = fixture.command();
            command.arg(if stdin_source {
                PathBuf::from("-")
            } else {
                fixture.source()
            });
            if non_interactive {
                command.arg("--non-interactive");
            }
            let mut child = command.spawn().expect("print should start");
            if stdin_source {
                child.stdin.as_mut().unwrap().write_all(PAYLOAD).unwrap();
            }
            drop(child.stdin.take());
            wait_for_exit(&mut child);
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stderr).contains("Printer: kitchen"));
            assert_eq!(fixture.received(), PAYLOAD);
        }
    }
}

#[test]
fn ambiguous_print_fails_before_reading_stdin_without_a_terminal() {
    for non_interactive in [false, true] {
        let fixture = Fixture::new(true);
        let mut command = fixture.command();
        command.arg("-");
        if non_interactive {
            command.arg("--non-interactive");
        }
        let mut child = command.spawn().expect("print should start");
        // Leave the receipt pipe open: target validation must not wait for EOF.
        wait_for_exit(&mut child);
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("multiple printers configured; pass --printer <NAME>")
        );
        fixture.assert_no_transfer();
    }
}

// util-linux script provides a controlling terminal for keyboard input, while
// cat supplies the binary receipt on a different pipe. This catches both the
// original stdin-is-terminal restriction and any accidental receipt corruption.
#[cfg(target_os = "linux")]
#[test]
fn piped_receipt_uses_terminal_picker_without_consuming_receipt_bytes() {
    run_terminal_selection("", Some(b"\x1b[B\r"), true);
}

#[cfg(target_os = "linux")]
#[test]
fn cancelling_piped_receipt_picker_sends_nothing() {
    run_terminal_selection("", Some(b"\x1b"), false);
}

#[cfg(target_os = "linux")]
#[test]
fn non_interactive_disables_picker_even_with_a_controlling_terminal() {
    run_terminal_selection("--non-interactive", None, false);
}

#[cfg(target_os = "linux")]
fn run_terminal_selection(flag: &str, keys: Option<&[u8]>, expect_print: bool) {
    use std::sync::mpsc;

    let fixture = Fixture::new(true);
    // Paths are environment values, never interpolated into shell code.
    let script = format!(
        "stty rows 30 cols 120; cat \"$RECEIPT\" | \"$ESCPOST\" print - --config \"$CONFIG\" {flag}"
    );
    let mut child = Command::new("script")
        .args([
            "--quiet",
            "--return",
            "--flush",
            "--command",
            &script,
            "/dev/null",
        ])
        .env("TERM", "xterm-256color")
        .env("DO_NOT_TRACK", "1")
        .env("ESCPOST", env!("CARGO_BIN_EXE_escpost"))
        .env("RECEIPT", fixture.source())
        .env("CONFIG", fixture.config())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the terminal session should start");
    let mut stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut all = Vec::new();
        let mut buffer = [0; 4096];
        while let Ok(count) = stdout.read(&mut buffer) {
            if count == 0 {
                break;
            }
            all.extend_from_slice(&buffer[..count]);
            let _ = sender.send(all.clone());
        }
        all
    });
    if let Some(keys) = keys {
        let deadline = Instant::now() + DEADLINE;
        loop {
            let output = receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|_| {
                    let _ = child.kill();
                    panic!("the piped receipt did not show a printer picker");
                });
            if String::from_utf8_lossy(&output).contains("Add a printer") {
                break;
            }
        }
        child.stdin.as_mut().unwrap().write_all(keys).unwrap();
    }
    wait_for_exit(&mut child);
    let status = child.wait().unwrap();
    let screen = String::from_utf8_lossy(&reader.join().unwrap()).into_owned();
    assert_eq!(status.success(), expect_print, "{screen}");
    if expect_print {
        assert!(screen.contains("Printer: kitchen"), "{screen}");
        assert_eq!(fixture.received(), PAYLOAD);
    } else {
        fixture.assert_no_transfer();
        if keys.is_none() {
            assert!(screen.contains("multiple printers configured"), "{screen}");
            assert!(!screen.contains("Add a printer"), "{screen}");
        }
    }
}

struct Fixture {
    directory: PathBuf,
    printer: TcpListener,
}

impl Fixture {
    fn new(multiple: bool) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("escpost-selection-{}-{unique}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let printer = TcpListener::bind("127.0.0.1:0").unwrap();
        printer.set_nonblocking(true).unwrap();
        let port = printer.local_addr().unwrap().port();
        let mut configuration =
            format!("[kitchen]\ntransport = \"network\"\nhost = \"127.0.0.1\"\nport = {port}\n");
        if multiple {
            // Alphabetically first, but deliberately not the selected printer.
            configuration
                .push_str("[counter]\ntransport = \"network\"\nhost = \"127.0.0.1\"\nport = 1\n");
        }
        fs::write(directory.join("printers.toml"), configuration).unwrap();
        fs::write(directory.join("receipt.bin"), PAYLOAD).unwrap();
        Self { directory, printer }
    }

    fn source(&self) -> PathBuf {
        self.directory.join("receipt.bin")
    }
    fn config(&self) -> PathBuf {
        self.directory.join("printers.toml")
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_escpost"));
        command
            .arg("print")
            .arg("--config")
            .arg(self.config())
            .env("DO_NOT_TRACK", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn received(&self) -> Vec<u8> {
        let (mut stream, _) = self
            .printer
            .accept()
            .expect("the selected printer should receive the job");
        stream.set_read_timeout(Some(DEADLINE)).unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        bytes
    }

    fn assert_no_transfer(&self) {
        assert_eq!(
            self.printer.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn wait_for_exit(child: &mut Child) {
    let deadline = Instant::now() + DEADLINE;
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("print did not finish within its test deadline");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
