use std::fs;
use std::io::Read;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[test]
fn print_help_contract_is_unchanged() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["print", "--help"])
        .output()
        .expect("the escpost command should finish");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("print help should be UTF-8"),
        "\
Send a known ESC/POS byte stream unchanged to a configured printer

Usage: escpost print [OPTIONS] <SOURCE>

Arguments:
  <SOURCE>  Raw ESC/POS file, hexadecimal file, case directory, or - for stdin

Options:
      --format <FORMAT>    Input representation [default: auto] [possible values: auto, binary, hex]
      --non-interactive    Never prompt for missing values
      --printer <PRINTER>  Configured printer name
      --config <FILE>      Read printer configuration from this exact file
  -h, --help               Print help
"
    );
}

#[test]
fn print_sends_exact_bytes_to_a_named_network_printer() {
    let directory = temporary_directory("named-network");
    let source = directory.join("receipt.bin");
    let configuration = directory.join("printers.toml");
    let expected = b"\x1b@Named network printer\n";
    fs::create_dir_all(&directory).expect("the test directory should be creatable");
    fs::write(&source, expected).expect("the ESC/POS source should be writable");
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("the loopback printer should bind");
    let port = listener
        .local_addr()
        .expect("the listener should have an address")
        .port();
    fs::write(
        &configuration,
        format!(
            "\
[kitchen]
transport = \"network\"
host = \"127.0.0.1\"
port = {port}
"
        ),
    )
    .expect("the printer configuration should be writable");
    let receiver = thread::spawn(move || {
        let (mut connection, _) = listener
            .accept()
            .expect("the named printer should receive a connection");
        let mut bytes = Vec::new();
        connection
            .read_to_end(&mut bytes)
            .expect("the print connection should close cleanly");
        bytes
    });

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "print",
            source.to_str().expect("the source path should be UTF-8"),
            "--printer",
            "kitchen",
            "--config",
            configuration
                .to_str()
                .expect("the configuration path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should finish");

    assert!(
        output.status.success(),
        "named network printing should succeed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        receiver.join().expect("the receiver should finish"),
        expected
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Printer: kitchen"));
    assert!(stderr.contains("Transport: network"));
    assert!(stderr.contains(&format!("Network target: 127.0.0.1:{port}")));
    assert!(stderr.contains(&format!("Bytes sent: {}", expected.len())));
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[test]
fn non_interactive_print_requires_a_named_printer() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cases/single-sheet/input.hex");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "print",
            source.to_str().expect("the source path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should finish");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(
        stderr.contains("printer is required; pass --printer <NAME>"),
        "the missing named target should be actionable:\n{stderr}"
    );
}

#[test]
fn unknown_named_printer_fails_without_attempting_a_connection() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cases/single-sheet/input.hex");
    let directory = temporary_directory("unknown-printer");
    fs::create_dir_all(&directory).expect("the test directory should be creatable");
    let configuration = directory.join("printers.toml");
    fs::write(
        &configuration,
        "\
[counter]
transport = \"network\"
host = \"127.0.0.1\"
port = 1
",
    )
    .expect("the printer configuration should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "print",
            source.to_str().expect("the source path should be UTF-8"),
            "--printer",
            "missing",
            "--config",
            configuration
                .to_str()
                .expect("the configuration path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should finish");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains(
        "printer \"missing\" is not configured; use `escpost printers list` to see available names"
    ));
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[test]
fn missing_input_file_is_reported_clearly() {
    let directory = temporary_directory("missing-input");
    fs::create_dir_all(&directory).expect("the test directory should be creatable");
    let configuration = directory.join("printers.toml");
    fs::write(
        &configuration,
        "\
[counter]
transport = \"network\"
host = \"127.0.0.1\"
port = 1
",
    )
    .expect("the printer configuration should be writable");
    let missing_source = directory.join("missing.hex");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "print",
            missing_source
                .to_str()
                .expect("the source path should be UTF-8"),
            "--printer",
            "counter",
            "--config",
            configuration
                .to_str()
                .expect("the configuration path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should finish");

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).expect("the error should be UTF-8"),
        format!(
            "error: ESC/POS input file {:?} was not found\n",
            missing_source
        )
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[test]
fn unknown_printer_is_rejected_before_an_invalid_source_is_loaded() {
    let directory = temporary_directory("unknown-printer-invalid-source");
    fs::create_dir_all(&directory).expect("the test directory should be creatable");
    let configuration = directory.join("printers.toml");
    fs::write(
        &configuration,
        "\
[counter]
transport = \"network\"
host = \"127.0.0.1\"
port = 1
",
    )
    .expect("the printer configuration should be writable");
    let missing_source = directory.join("missing.hex");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "print",
            missing_source
                .to_str()
                .expect("the source path should be UTF-8"),
            "--printer",
            "missing",
            "--config",
            configuration
                .to_str()
                .expect("the configuration path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should finish");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(
        stderr.contains(
            "printer \"missing\" is not configured; use `escpost printers list` to see available names"
        ),
        "the configured target must be validated before the source:\n{stderr}"
    );
    assert!(
        !stderr.contains("could not read ESC/POS input"),
        "the invalid source must not be inspected after target preflight fails:\n{stderr}"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[test]
fn unknown_printer_is_rejected_without_waiting_for_stdin_eof() {
    let directory = temporary_directory("unknown-printer-open-stdin");
    fs::create_dir_all(&directory).expect("the test directory should be creatable");
    let configuration = directory.join("printers.toml");
    fs::write(
        &configuration,
        "\
[counter]
transport = \"network\"
host = \"127.0.0.1\"
port = 1
",
    )
    .expect("the printer configuration should be writable");

    let mut child = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "print",
            "-",
            "--printer",
            "missing",
            "--config",
            configuration
                .to_str()
                .expect("the configuration path should be UTF-8"),
            "--non-interactive",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the escpost command should start");
    let held_open_stdin = child
        .stdin
        .take()
        .expect("the child's stdin should remain open during preflight");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if child
            .try_wait()
            .expect("the child status should be readable")
            .is_some()
        {
            break;
        }
        if Instant::now() >= deadline {
            child
                .kill()
                .expect("the blocked command should be stoppable");
            drop(held_open_stdin);
            let output = child
                .wait_with_output()
                .expect("the stopped command should be reapable");
            panic!(
                "print waited for stdin before validating its target:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
    drop(held_open_stdin);
    let output = child
        .wait_with_output()
        .expect("the preflighted command should finish");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(
        stderr.contains(
            "printer \"missing\" is not configured; use `escpost printers list` to see available names"
        ),
        "the command should reject the target while stdin is still open:\n{stderr}"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

fn temporary_directory(case: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock should be after the Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "escpost-print-command-{case}-{}-{unique}",
        std::process::id()
    ))
}
