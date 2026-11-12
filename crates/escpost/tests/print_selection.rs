use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PAYLOAD: &[u8] = b"\x1b@Receipt\x00\xff\n\n";
const DEADLINE: Duration = Duration::from_secs(10);

#[test]
fn raw_stdin_streams_successive_chunks_before_eof_on_one_connection() {
    for explicit_stdin in [false, true] {
        let fixture = Fixture::new(false);
        let mut command = fixture.command();
        if explicit_stdin {
            command.args(["-", "--format", "binary"]);
        }
        let mut child = command.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (messages, output) = std::sync::mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                if messages.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        // Destination details must be visible before any input arrives, not
        // merely appear in the right order after the producer closes stdin.
        for expected in [
            "Printer: kitchen".to_owned(),
            "Transport: network".to_owned(),
            format!("Network target: {}", fixture.printer.local_addr().unwrap()),
            "Streaming stdin; Ctrl+C to stop.".to_owned(),
        ] {
            let line = output.recv_timeout(DEADLINE).unwrap_or_else(|error| {
                let _ = child.kill();
                let _ = child.wait();
                panic!("missing output before stdin EOF: {expected}: {error}");
            });
            assert_eq!(line, expected);
        }
        let deadline = Instant::now() + DEADLINE;
        let mut stream = loop {
            match fixture.printer.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        panic!("printing waited for stdin EOF before connecting");
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("{error}"),
            }
        };
        // Windows accepted sockets inherit the listener's nonblocking mode.
        stream.set_nonblocking(false).unwrap();
        stream.set_read_timeout(Some(DEADLINE)).unwrap();
        // The connection is established while stdin is still empty. Whether
        // bytes are already available must not decide which source is used.
        stdin.write_all(PAYLOAD).unwrap();
        let mut bytes = vec![0; PAYLOAD.len()];
        stream.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, PAYLOAD);
        assert!(child.try_wait().unwrap().is_none());
        stdin.write_all(b"second\x1dV\0").unwrap();
        let mut second = [0; 9];
        stream.read_exact(&mut second).unwrap();
        assert_eq!(&second, b"second\x1dV\0");
        assert!(output.try_recv().is_err(), "no byte summary before EOF");
        drop(stdin);
        wait_for_exit(&mut child);
        assert!(child.wait().unwrap().success());
        reader.join().unwrap();
        assert_eq!(
            output.into_iter().collect::<Vec<_>>(),
            [format!("Bytes sent: {}", PAYLOAD.len() + second.len())]
        );
        assert_eq!(stream.read(&mut bytes).unwrap(), 0);
        fixture.assert_no_transfer();
    }
}

#[test]
fn empty_stdin_finishes_without_sending_bytes() {
    let fixture = Fixture::new(false);
    let mut child = fixture.command().spawn().unwrap();
    drop(child.stdin.take());
    wait_for_exit(&mut child);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("Bytes sent: 0"));
    assert!(fixture.received().is_empty());
}

#[test]
fn valid_hex_stdin_is_decoded_only_after_eof() {
    let fixture = Fixture::new(false);
    let mut child = fixture.command().args(["--format", "hex"]).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"1b 40 ").unwrap();
    thread::sleep(Duration::from_millis(30));
    fixture.assert_no_transfer();
    stdin.write_all(b"ff 00 0a").unwrap();
    drop(stdin);
    wait_for_exit(&mut child);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fixture.received(), [27, 64, 255, 0, 10]);
}

#[test]
fn malformed_hex_stdin_is_rejected_without_sending_a_valid_prefix() {
    let fixture = Fixture::new(false);
    let mut child = fixture.command().args(["--format", "hex"]).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"1b 40 ").unwrap();
    thread::sleep(Duration::from_millis(30));
    fixture.assert_no_transfer();
    stdin.write_all(b"zz").unwrap();
    drop(stdin);
    wait_for_exit(&mut child);
    assert!(!child.wait_with_output().unwrap().status.success());
    fixture.assert_no_transfer();
}

#[test]
fn sole_printer_is_used_for_files_and_pipes_with_or_without_non_interactive() {
    for source in ["file", "stdin", "implicit-stdin"] {
        for non_interactive in [false, true] {
            let fixture = Fixture::new(false);
            let mut command = fixture.command();
            match source {
                "file" => {
                    command.arg(fixture.source());
                }
                "stdin" => {
                    command.arg("-");
                }
                _ => {}
            }
            if non_interactive {
                command.arg("--non-interactive");
            }
            let mut child = command.spawn().expect("print should start");
            if source != "file" {
                child.stdin.as_mut().unwrap().write_all(PAYLOAD).unwrap();
            } else {
                child
                    .stdin
                    .as_mut()
                    .unwrap()
                    .write_all(b"ignored stdin")
                    .unwrap();
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

#[cfg(target_os = "linux")]
#[test]
fn missing_source_at_a_terminal_fails_before_printer_selection() {
    let fixture = Fixture::new(true);
    let mut child = Command::new("script")
        .args([
            "--quiet",
            "--return",
            "--command",
            "\"$ESCPOST\" print --config \"$CONFIG\"",
            "/dev/null",
        ])
        .env("ESCPOST", env!("CARGO_BIN_EXE_escpost"))
        .env("CONFIG", fixture.config())
        .env("DO_NOT_TRACK", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_exit(&mut child);
    let output = child.wait_with_output().unwrap();
    let screen = String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success());
    assert!(screen.contains("source is required"), "{screen}");
    assert!(!screen.contains("Add a printer"), "{screen}");
    fixture.assert_no_transfer();
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
        stream.set_nonblocking(false).unwrap();
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
