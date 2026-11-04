//! The usage event is sent from the root dispatcher, so these tests run the
//! real binary against a loopback HTTP listener that records request bodies.

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const INSTALLATION_ID: &str = "11111111-2222-4333-8444-555555555555";
const EXAMPLE_JOB: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../example-jobs/cafe-order-voucher.hex"
);

#[test]
fn a_consented_installation_reports_one_event_with_names_but_no_values() {
    let directory = TemporaryDirectory::new("consented-render");
    write_consent(&directory.path, true);
    let listener = CaptureListener::start(1);
    let rendered = directory.path.join("rendered-sheets");

    let output = escpost(&directory.path, Some(listener.endpoint()))
        .args([
            "render",
            EXAMPLE_JOB,
            "--profile",
            "REFERENCE",
            "--output-dir",
        ])
        .arg(&rendered)
        .arg("--non-interactive")
        .output()
        .expect("the escpost command should finish");

    assert_success(&output);
    let bodies = listener.finish();
    assert_eq!(bodies.len(), 1, "exactly one event per invocation");
    let body: serde_json::Value =
        serde_json::from_str(&bodies[0]).expect("the event body should be JSON");
    assert_eq!(body["event"], "escpost_cli_command");
    assert_eq!(body["distinct_id"], INSTALLATION_ID);
    let properties = &body["properties"];
    assert_eq!(properties["$process_person_profile"], false);
    assert_eq!(properties["source"], "escpost-cli");
    assert_eq!(properties["command"], "render");
    assert_eq!(
        properties["flags"],
        serde_json::json!(["non-interactive", "output-dir", "profile"])
    );
    assert_eq!(properties["outcome"], "ok");
    assert!(properties["duration_ms"].is_u64());
    assert_eq!(properties["version"], env!("CARGO_PKG_VERSION"));
    assert!(properties.get("first_run").is_none());
    // Values the user typed must not appear anywhere in the body.
    assert!(
        !bodies[0].contains("REFERENCE"),
        "profile value leaked:\n{}",
        bodies[0]
    );
    assert!(
        !bodies[0].contains("cafe-order"),
        "input path leaked:\n{}",
        bodies[0]
    );
    assert!(
        !bodies[0].contains("rendered-sheets"),
        "output path leaked:\n{}",
        bodies[0]
    );
}

#[test]
fn a_failed_command_reports_its_error_kind() {
    let directory = TemporaryDirectory::new("consented-failure");
    write_consent(&directory.path, true);
    let listener = CaptureListener::start(1);

    let output = escpost(&directory.path, Some(listener.endpoint()))
        .args(["--non-interactive", "render", EXAMPLE_JOB, "--output-dir"])
        .arg(directory.path.join("rendered-sheets"))
        .output()
        .expect("the escpost command should finish");

    assert!(
        !output.status.success(),
        "render without a profile must fail"
    );
    let bodies = listener.finish();
    assert_eq!(bodies.len(), 1);
    let body: serde_json::Value = serde_json::from_str(&bodies[0]).expect("JSON body");
    assert_eq!(body["properties"]["outcome"], "missing_profile");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("telemetry"),
        "telemetry must stay silent:\n{stderr}"
    );
}

#[test]
fn an_undecided_non_interactive_run_sends_nothing_and_writes_nothing() {
    let directory = TemporaryDirectory::new("undecided");
    let listener = CaptureListener::start(0);

    let output = escpost(&directory.path, Some(listener.endpoint()))
        .args(["--non-interactive", "profiles", "list"])
        .output()
        .expect("the escpost command should finish");

    assert_success(&output);
    assert!(listener.finish().is_empty());
    assert!(
        !directory.path.join("telemetry.toml").exists(),
        "no decision may be written without asking"
    );
}

#[test]
fn do_not_track_beats_a_stored_opt_in() {
    let directory = TemporaryDirectory::new("do-not-track");
    write_consent(&directory.path, true);
    let listener = CaptureListener::start(0);

    let output = escpost(&directory.path, Some(listener.endpoint()))
        .env("DO_NOT_TRACK", "1")
        .args(["--non-interactive", "profiles", "list"])
        .output()
        .expect("the escpost command should finish");

    assert_success(&output);
    assert!(listener.finish().is_empty());
}

#[test]
fn an_unreachable_endpoint_changes_neither_exit_code_nor_output() {
    let directory = TemporaryDirectory::new("unreachable");
    write_consent(&directory.path, true);
    let closed_port = unused_loopback_port();

    let reported = escpost(
        &directory.path,
        Some(format!("http://127.0.0.1:{closed_port}/")),
    )
    .args(["--non-interactive", "profiles", "get", "REFERENCE"])
    .output()
    .expect("the escpost command should finish");
    let silent = escpost(&directory.path, None)
        .env("DO_NOT_TRACK", "1")
        .args(["--non-interactive", "profiles", "get", "REFERENCE"])
        .output()
        .expect("the escpost command should finish");

    assert_success(&reported);
    assert_eq!(reported.status, silent.status);
    assert_eq!(reported.stdout, silent.stdout);
    assert_eq!(reported.stderr, silent.stderr);
}

#[test]
fn a_malformed_endpoint_is_swallowed_too() {
    // Review focus 5.
    let directory = TemporaryDirectory::new("malformed-endpoint");
    write_consent(&directory.path, true);

    let output = escpost(&directory.path, Some("not a url".to_owned()))
        .args(["--non-interactive", "profiles", "get", "REFERENCE"])
        .output()
        .expect("the escpost command should finish");

    assert_success(&output);
    assert!(!String::from_utf8_lossy(&output.stderr).contains("telemetry"));
}

#[test]
fn an_unwritable_configuration_directory_does_not_break_the_command() {
    // Review focus 2: ESCPOST_CONFIG_DIR points inside a regular file.
    let directory = TemporaryDirectory::new("unwritable");
    let blocker = directory.path.join("blocker");
    fs::write(&blocker, "").expect("fixture should be written");

    let output = escpost(&blocker.join("escpost"), None)
        .args(["--non-interactive", "profiles", "get", "REFERENCE"])
        .output()
        .expect("the escpost command should finish");

    assert_success(&output);
}

#[test]
fn serve_reports_started_before_it_blocks() {
    let directory = TemporaryDirectory::new("serve-started");
    write_consent(&directory.path, true);
    let listener = CaptureListener::start(1);
    let port = unused_loopback_port();

    let mut child = escpost(&directory.path, Some(listener.endpoint()))
        .args([
            "--non-interactive",
            "serve",
            "--listen",
            &port.to_string(),
            "--no-open",
            "--idle-timeout",
            "1",
        ])
        .spawn()
        .expect("the escpost command should start");

    let bodies = listener.finish();
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(bodies.len(), 1);
    let body: serde_json::Value = serde_json::from_str(&bodies[0]).expect("JSON body");
    assert_eq!(body["properties"]["command"], "serve");
    assert_eq!(body["properties"]["outcome"], "started");
    assert!(body["properties"].get("duration_ms").is_none());
}

// ---- helpers ----

fn escpost(config_directory: &Path, endpoint: Option<String>) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_escpost"));
    command
        .env("ESCPOST_CONFIG_DIR", config_directory)
        .env_remove("DO_NOT_TRACK")
        .env_remove("ESCPOST_TELEMETRY")
        .env_remove("ESCPOST_TELEMETRY_DEBUG");
    match endpoint {
        Some(endpoint) => command.env("ESCPOST_TELEMETRY_ENDPOINT", endpoint),
        None => command.env_remove("ESCPOST_TELEMETRY_ENDPOINT"),
    };
    command
}

fn write_consent(directory: &Path, enabled: bool) {
    fs::write(
        directory.join("telemetry.toml"),
        format!(
            "enabled = {enabled}\ninstallation_id = \"{INSTALLATION_ID}\"\ndecided_at = \"2026-11-04T16:02:11Z\"\n"
        ),
    )
    .expect("the consent fixture should be written");
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn unused_loopback_port() -> u16 {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .expect("a loopback port should bind");
    listener
        .local_addr()
        .expect("the listener should have an address")
        .port()
}

/// A loopback HTTP/1.1 listener that answers every POST with 200 and keeps
/// the bodies. `expected` bounds how long `finish` waits.
struct CaptureListener {
    port: u16,
    handle: JoinHandle<Vec<String>>,
}

impl CaptureListener {
    fn start(expected: usize) -> Self {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .expect("the capture listener should bind");
        listener
            .set_nonblocking(true)
            .expect("the capture listener should be non-blocking");
        let port = listener
            .local_addr()
            .expect("the listener should have an address")
            .port();
        let handle = thread::spawn(move || {
            let mut bodies = Vec::new();
            // Wait generously for the expected requests, but only briefly for
            // unexpected extra ones so "zero requests" tests stay fast.
            let deadline = Instant::now() + Duration::from_secs(10);
            let settle = Duration::from_millis(500);
            let mut last_activity = Instant::now();
            loop {
                match listener.accept() {
                    Ok((stream, _)) => {
                        bodies.push(read_request_body(stream));
                        last_activity = Instant::now();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        let done_waiting =
                            bodies.len() >= expected && last_activity.elapsed() > settle;
                        if done_waiting || Instant::now() > deadline {
                            break;
                        }
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(error) => panic!("accept failed: {error}"),
                }
            }
            bodies
        });
        Self { port, handle }
    }

    fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}/i/v0/e/", self.port)
    }

    fn finish(self) -> Vec<String> {
        self.handle
            .join()
            .expect("the capture listener thread should finish")
    }
}

fn read_request_body(mut stream: TcpStream) -> String {
    stream
        .set_nonblocking(false)
        .expect("the accepted connection should block");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("the request should have a read deadline");
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let read = stream
            .read(&mut chunk)
            .expect("the request head should be readable");
        assert!(read > 0, "the client closed before sending a request head");
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    assert!(
        head.starts_with("POST /i/v0/e/ "),
        "unexpected request head:\n{head}"
    );
    let content_length: usize = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length").then(|| {
                value
                    .trim()
                    .parse()
                    .expect("Content-Length should be a number")
            })
        })
        .expect("the request should carry Content-Length");
    while buffer.len() < header_end + content_length {
        let read = stream
            .read(&mut chunk)
            .expect("the request body should be readable");
        assert!(read > 0, "the client closed mid-body");
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body =
        String::from_utf8_lossy(&buffer[header_end..header_end + content_length]).to_string();
    let _ =
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
    body
}

struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn new(case: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the system clock should be after the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "escpost-telemetry-{case}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("the temporary directory should be creatable");
        Self { path }
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
