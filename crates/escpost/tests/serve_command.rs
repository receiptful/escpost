use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn serve_help_contract_is_unchanged() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["serve", "--help"])
        .output()
        .expect("the escpost command should finish");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("serve help should be UTF-8"),
        "\
Capture RAW TCP print jobs and preview them in the web viewer

Usage: escpost serve [OPTIONS]

Options:
      --non-interactive          Never prompt for missing values
      --profile <PROFILE>        Printer profile used to render captured jobs [default: REFERENCE]
      --listen <LISTEN>          Address for the RAW TCP printer. When omitted, the first free loopback port from 9100 through 9109 is used
      --web-listen <WEB_LISTEN>  Address for the web viewer. When omitted, the first free loopback port from 9000 through 9099 is used
      --idle-timeout <SECONDS>   Complete a held-open connection's job after this many seconds of silence. Use 0 to disable and end a job only when the connection closes [default: 20]
      --scale <N>                Preview pixel density: 1 to 3 subpixels per dot. 1 is dot resolution [default: 3]
      --antialias [<ANTIALIAS>]  Anti-alias glyph edges into a grayscale preview (cosmetic; never what a printer emits). Pass --antialias=false for faithful 1-bit dots [default: true] [possible values: true, false]
      --no-open                  Do not open the web viewer in the default browser on startup. Auto-open is also skipped with --non-interactive, without a terminal, or when the BROWSER=none or CI environment variables are set
  -h, --help                     Print help
"
    );
}

#[test]
fn serve_rejects_an_unsupported_scale_before_opening_listeners() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["serve", "--scale", "0", "--non-interactive"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the escpost command should start");
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
            let output = child
                .wait_with_output()
                .expect("the stopped command should be reapable");
            panic!(
                "serve opened listeners instead of rejecting the scale:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child
        .wait_with_output()
        .expect("the rejected command should finish");

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: render scale must be between 1 and 3, got 0\n"
    );
}

#[test]
fn serve_captures_a_raw_job_and_previews_its_sheets() {
    let mut child = start_serve_on_ephemeral_ports();
    let (raw_port, web_port) = read_listen_ports(&mut child);

    wait_until_listening(&mut child, raw_port);
    wait_until_listening(&mut child, web_port);

    let waiting_response = http_get_bytes(web_port, "/api/jobs/current");
    let waiting: serde_json::Value = serde_json::from_slice(response_body(&waiting_response))
        .expect("the waiting job response should be JSON");
    assert_eq!(waiting["job"], serde_json::Value::Null);
    assert_eq!(waiting["receiving"], false);
    assert_eq!(waiting["profile"], "REFERENCE");
    assert!(waiting["hint"].as_str().is_some());

    // A RAW/AppSocket client sends one job and closes the connection, which is
    // the default end-of-job boundary.
    send_raw_job(raw_port, b"Captured over RAW\n");

    let metadata = wait_for_first_job(web_port);
    let sheets = metadata["sheets"]
        .as_array()
        .expect("sheets should be an array");
    assert!(!sheets.is_empty(), "the captured job should render a sheet");
    assert_eq!(metadata["profile"], "REFERENCE");

    // A job ended by the client closing the connection is labelled "closed".
    assert_eq!(metadata["completion"], "closed");
    // Its completion time is reported so the viewer always shows a status.
    assert!(
        metadata["completed_at"].as_u64().is_some(),
        "a completed job should carry a completion timestamp"
    );

    let png = response_body(&http_get_bytes(web_port, "/sheets/1.png")).to_vec();
    stop(&mut child);
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn api_status_reports_virtual_printer_and_only_successful_captured_jobs() {
    let mut child = start_serve_on_ephemeral_ports();
    let (raw_port, web_port) = read_listen_ports(&mut child);
    wait_until_listening(&mut child, raw_port);
    wait_until_listening(&mut child, web_port);

    let raw_address = format!("127.0.0.1:{raw_port}");
    let initial_response = http_get_bytes(web_port, "/api/status");
    assert_eq!(
        response_header(&initial_response, "cache-control"),
        Some("no-store")
    );
    assert!(matches!(
        response_header(&initial_response, "content-type"),
        Some(value) if value.starts_with("application/json")
    ));
    let initial: serde_json::Value = serde_json::from_slice(response_body(&initial_response))
        .expect("the status response should be JSON");
    assert_eq!(initial["virtual_printer"]["state"], "ready");
    assert_eq!(initial["virtual_printer"]["address"], raw_address);
    assert_eq!(initial["jobs_processed"], 0);

    let generation = render_metadata(web_port)["generation"]
        .as_u64()
        .expect("the initial render generation should be numeric");
    send_raw_job(raw_port, b"Count this captured job\n");
    wait_for_generation_change(web_port, generation);
    assert_eq!(status_metadata(web_port)["jobs_processed"], 1);

    // An unsupported FS byte fails rendering, so it is not a processed job.
    send_raw_job(raw_port, b"Do not count this\n\x1c");
    wait_for_render_error(web_port);
    let status = status_metadata(web_port);
    stop(&mut child);

    assert_eq!(status["jobs_processed"], 1);
}

#[test]
fn status_event_reports_receiving_then_ready_for_an_unsuccessful_capture() {
    let mut child = start_serve(&[
        "--profile",
        "REFERENCE",
        "--idle-timeout",
        "0",
        "--listen",
        "127.0.0.1:0",
        "--web-listen",
        "127.0.0.1:0",
    ]);
    let (raw_port, web_port) = read_listen_ports(&mut child);
    wait_until_listening(&mut child, raw_port);
    wait_until_listening(&mut child, web_port);

    let mut events = open_status_events(web_port);
    assert_eq!(
        next_status_event(&mut events)["virtual_printer"]["state"],
        "ready"
    );

    let mut raw = open_raw_connection(raw_port);
    raw.write_all(b"\x1c")
        .expect("the unsupported byte should be writable");
    raw.flush().expect("the unsupported byte should flush");
    assert_eq!(
        next_status_event(&mut events)["virtual_printer"]["state"],
        "receiving"
    );

    drop(raw);
    let completed = next_status_event(&mut events);
    stop(&mut child);

    assert_eq!(completed["virtual_printer"]["state"], "ready");
    assert_eq!(completed["jobs_processed"], 0);
}

#[test]
fn status_event_keeps_other_subscribers_live_and_reports_a_successful_job() {
    let mut child = start_serve_on_ephemeral_ports();
    let (raw_port, web_port) = read_listen_ports(&mut child);
    wait_until_listening(&mut child, raw_port);
    wait_until_listening(&mut child, web_port);

    let mut events = open_status_events(web_port);
    let initial = next_status_event(&mut events);
    assert_eq!(initial["virtual_printer"]["state"], "ready");
    assert_eq!(initial["jobs_processed"], 0);

    let mut disconnected = open_status_events(web_port);
    let disconnected_initial = next_status_event(&mut disconnected);
    assert_eq!(disconnected_initial, initial);
    drop(disconnected);

    send_raw_job(raw_port, b"Count this streamed job\n");
    let completed = loop {
        let event = next_status_event(&mut events);
        if event["jobs_processed"] == 1 && event["virtual_printer"]["state"] == "ready" {
            break event;
        }
    };
    stop(&mut child);

    assert_eq!(completed["jobs_processed"], 1);
}

#[test]
fn serve_finalizes_a_held_open_connection_after_the_idle_timeout() {
    let mut child = start_serve(&[
        "--profile",
        "REFERENCE",
        "--idle-timeout",
        "0.3",
        "--listen",
        "127.0.0.1:0",
        "--web-listen",
        "127.0.0.1:0",
    ]);
    let (raw_port, web_port) = read_listen_ports(&mut child);
    wait_until_listening(&mut child, raw_port);
    wait_until_listening(&mut child, web_port);

    // Send a receipt but deliberately keep the connection open. The idle timeout
    // must finalize the job anyway.
    let mut stream = open_raw_connection(raw_port);
    stream
        .write_all(b"Held-open receipt\n")
        .expect("the receipt should be writable");
    stream.flush().expect("the receipt should flush");

    let metadata = wait_for_first_job(web_port);
    assert!(
        !metadata["sheets"]
            .as_array()
            .expect("sheets should be an array")
            .is_empty(),
        "the held-open job should render once the idle timeout elapses"
    );
    assert_eq!(metadata["completion"], "timeout");

    drop(stream);
    stop(&mut child);
}

#[test]
fn serve_offers_the_captured_raw_input_for_download() {
    let mut child = start_serve_on_ephemeral_ports();
    let (raw_port, web_port) = read_listen_ports(&mut child);
    wait_until_listening(&mut child, raw_port);
    wait_until_listening(&mut child, web_port);

    let sent = b"Downloadable raw input\n";
    send_raw_job(raw_port, sent);
    let metadata = wait_for_first_job(web_port);
    assert_eq!(metadata["input_available"], true);

    let current_response = http_get_bytes(web_port, "/api/jobs/current");
    let current: serde_json::Value = serde_json::from_slice(response_body(&current_response))
        .expect("the current job response should be JSON");
    let input_url = current["job"]["input_url"]
        .as_str()
        .expect("captured input should have a stable download URL");
    let response = http_get_bytes(web_port, input_url);
    stop(&mut child);

    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert_eq!(
        response_header(&response, "cache-control"),
        Some("no-store")
    );
    assert!(
        String::from_utf8_lossy(&response)
            .to_ascii_lowercase()
            .contains("content-disposition: attachment"),
        "the raw input should download as an attachment"
    );
    // The download is the exact captured bytes, not the rendered output.
    assert_eq!(response_body(&response), sent);
}

#[test]
fn serve_flags_a_connection_that_is_still_receiving() {
    // Disable the idle timeout so the job stays in-progress until we close.
    let mut child = start_serve(&[
        "--profile",
        "REFERENCE",
        "--idle-timeout",
        "0",
        "--listen",
        "127.0.0.1:0",
        "--web-listen",
        "127.0.0.1:0",
    ]);
    let (raw_port, web_port) = read_listen_ports(&mut child);
    wait_until_listening(&mut child, raw_port);
    wait_until_listening(&mut child, web_port);

    let mut stream = open_raw_connection(raw_port);
    stream
        .write_all(b"Half a receipt so far\n")
        .expect("the partial receipt should be writable");
    stream.flush().expect("the partial receipt should flush");

    // A connection holding buffered bytes is reported as receiving by the
    // runtime status endpoint, not only by the legacy render response.
    let status = wait_until_status_receiving(web_port, true);
    assert_eq!(status["virtual_printer"]["state"], "receiving");

    // Closing finalizes the job and clears the receiving flag.
    drop(stream);
    let status = wait_until_status_receiving(web_port, false);
    let metadata = wait_until_receiving(web_port, false);
    stop(&mut child);

    assert!(
        !metadata["sheets"]
            .as_array()
            .expect("sheets should be an array")
            .is_empty(),
        "the finalized job should render"
    );
    assert_eq!(metadata["completion"], "closed");
    assert_eq!(status["virtual_printer"]["state"], "ready");
}

#[test]
fn serve_replaces_the_preview_with_the_most_recent_job() {
    let mut child = start_serve_on_ephemeral_ports();
    let (raw_port, web_port) = read_listen_ports(&mut child);
    wait_until_listening(&mut child, raw_port);
    wait_until_listening(&mut child, web_port);

    send_raw_job(raw_port, b"First job\n");
    wait_for_first_job(web_port);
    let first = response_body(&http_get_bytes(web_port, "/sheets/1.png")).to_vec();

    send_raw_job(raw_port, b"A visibly different second job\n");
    let deadline = Instant::now() + Duration::from_secs(5);
    let second = loop {
        let candidate = response_body(&http_get_bytes(web_port, "/sheets/1.png")).to_vec();
        if candidate != first {
            break candidate;
        }
        assert!(
            Instant::now() < deadline,
            "the preview did not advance to the second job"
        );
        thread::sleep(Duration::from_millis(50));
    };
    stop(&mut child);
    assert_eq!(&second[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn serve_viewer_shows_instructions_before_the_first_job() {
    let mut child = start_serve_on_ephemeral_ports();
    let (raw_port, web_port) = read_listen_ports(&mut child);
    wait_until_listening(&mut child, web_port);

    let metadata = render_metadata(web_port);
    stop(&mut child);

    assert_eq!(
        metadata["sheets"]
            .as_array()
            .expect("sheets should be an array")
            .len(),
        0,
        "no job has been captured yet"
    );
    let hint = metadata["hint"]
        .as_str()
        .expect("an idle viewer should carry a waiting hint");
    assert!(
        hint.contains(&format!("127.0.0.1:{raw_port}")),
        "the hint should tell the developer where to send data:\n{hint}"
    );
    // The profile is known at startup, so it is reported before any job.
    assert_eq!(metadata["profile"], "REFERENCE");
}

#[test]
fn serve_reassembles_a_job_sent_one_byte_at_a_time() {
    let mut child = start_serve_on_ephemeral_ports();
    let (raw_port, web_port) = read_listen_ports(&mut child);
    wait_until_listening(&mut child, raw_port);
    wait_until_listening(&mut child, web_port);

    // Deliver the receipt one byte per write so the server must reassemble the
    // job across many reads, as it would with a fragmenting client.
    send_raw_job_one_byte_at_a_time(raw_port, b"Fragmented receipt\n");

    let metadata = wait_for_first_job(web_port);
    assert!(
        !metadata["sheets"]
            .as_array()
            .expect("sheets should be an array")
            .is_empty(),
        "a byte-by-byte job should still render"
    );
    let png = response_body(&http_get_bytes(web_port, "/sheets/1.png")).to_vec();
    stop(&mut child);
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn serve_defaults_the_profile_to_reference() {
    // No --profile: a virtual printer should preview with REFERENCE.
    let mut child = start_serve(&["--listen", "127.0.0.1:0", "--web-listen", "127.0.0.1:0"]);
    let (raw_port, web_port) = read_listen_ports(&mut child);
    wait_until_listening(&mut child, raw_port);
    wait_until_listening(&mut child, web_port);

    send_raw_job(raw_port, b"Default profile\n");
    let metadata = wait_for_first_job(web_port);
    stop(&mut child);

    assert_eq!(metadata["profile"], "REFERENCE");
}

#[test]
fn serve_reports_a_render_error_without_a_sheet() {
    // A byte the parser cannot handle (FS, 0x1c) fails the render outright. The
    // viewer reads `error` with no sheets and must report the failure rather
    // than keep waiting for the first job.
    let mut child = start_serve_on_ephemeral_ports();
    let mut stderr = BufReader::new(
        child
            .stderr
            .take()
            .expect("the serve command stderr should be piped"),
    );
    let (raw_port, web_port) = read_listen_ports_from(&mut stderr);
    wait_until_listening(&mut child, raw_port);
    wait_until_listening(&mut child, web_port);

    // 0x1c (FS) is not a supported ESC/POS data byte.
    send_raw_job(raw_port, b"Broken\n\x1c");

    let metadata = wait_for_render_error(web_port);
    stop(&mut child);
    let mut remaining_stderr = String::new();
    stderr
        .read_to_string(&mut remaining_stderr)
        .expect("the render warning should be readable");

    assert_eq!(
        metadata["sheets"]
            .as_array()
            .expect("sheets should be an array")
            .len(),
        0,
        "a failed render produces no sheet"
    );
    let error = metadata["error"]
        .as_str()
        .expect("the render error should be reported");
    assert_eq!(error, "unsupported data byte 0x1c at byte offset 7");
    assert_eq!(
        remaining_stderr,
        "Press Ctrl+C to stop.\n\
warning: could not render captured job: unsupported data byte 0x1c at byte offset 7\n"
    );
}

#[test]
fn serve_splits_and_warns_on_a_cut_without_a_cutter() {
    // NT-5890K has no cutter, so a full cut cannot be performed. The job still
    // renders — split at the boundary — and the API surfaces a warning rather
    // than failing the render.
    let mut child = start_serve(&[
        "--profile",
        "NT-5890K",
        "--listen",
        "127.0.0.1:0",
        "--web-listen",
        "127.0.0.1:0",
    ]);
    let (raw_port, web_port) = read_listen_ports(&mut child);
    wait_until_listening(&mut child, raw_port);
    wait_until_listening(&mut child, web_port);

    // Two receipts separated by a full cut the profile cannot perform.
    send_raw_job(raw_port, b"First\n\x1dV\x00Second\n");

    let metadata = wait_for_first_job(web_port);
    stop(&mut child);

    assert_eq!(
        metadata["sheets"]
            .as_array()
            .expect("sheets should be an array")
            .len(),
        2,
        "the cut should still split the preview into two receipts"
    );
    let warnings = metadata["warnings"]
        .as_array()
        .expect("warnings should be an array");
    assert_eq!(
        warnings.len(),
        1,
        "the uncuttable cut should record one warning"
    );
    assert!(
        warnings[0]
            .as_str()
            .unwrap_or_default()
            .contains("not physically"),
        "the warning should explain the cut was not performed:\n{warnings:?}"
    );
}

fn start_serve(arguments: &[&str]) -> Child {
    Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["--non-interactive", "serve"])
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the escpost command should start")
}

/// Bind both listeners to operating-system-selected ports so parallel tests do
/// not contend for the conventional 9100/9000 defaults.
fn start_serve_on_ephemeral_ports() -> Child {
    start_serve(&[
        "--profile",
        "REFERENCE",
        "--listen",
        "127.0.0.1:0",
        "--web-listen",
        "127.0.0.1:0",
    ])
}

/// Both listeners bind an operating-system-selected port, so the actual ports
/// are read back from the status the server prints.
fn read_listen_ports(child: &mut Child) -> (u16, u16) {
    let mut stderr = BufReader::new(
        child
            .stderr
            .take()
            .expect("the serve command stderr should be piped"),
    );
    let ports = read_listen_ports_from(&mut stderr);

    // Keep the stderr pipe open and drained for the rest of the run. Dropping it
    // here would close the read end, so the server's next status line would hit
    // a broken pipe and abort. The thread ends at EOF when the child is stopped.
    thread::spawn(move || {
        let mut discard = String::new();
        let _ = stderr.read_to_string(&mut discard);
    });

    ports
}

fn read_listen_ports_from(stderr: &mut impl BufRead) -> (u16, u16) {
    let mut raw = None;
    let mut web = None;
    while raw.is_none() || web.is_none() {
        let mut line = String::new();
        let read = stderr
            .read_line(&mut line)
            .expect("serve status should be readable");
        assert!(read != 0, "serve exited before reporting its listen ports");
        if let Some(port) = line
            .trim()
            .strip_prefix("RAW printer: 127.0.0.1:")
            .and_then(|value| value.parse::<u16>().ok())
        {
            raw = Some(port);
        }
        if let Some(port) = line
            .trim()
            .strip_prefix("Web viewer: http://127.0.0.1:")
            .and_then(|value| value.strip_suffix('/'))
            .and_then(|value| value.parse::<u16>().ok())
        {
            web = Some(port);
        }
    }

    (raw.unwrap(), web.unwrap())
}

fn send_raw_job(port: u16, bytes: &[u8]) {
    // The escpost service uses host networking, so a loopback connection can be
    // refused transiently while the host is busy. Retry until a deadline.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match raw_send_once(port, bytes) {
            Ok(()) => return,
            Err(error) => {
                assert!(
                    Instant::now() < deadline,
                    "the RAW printer never accepted data: {error}"
                );
                thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

fn raw_send_once(port: u16, bytes: &[u8]) -> std::io::Result<()> {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
    stream.write_all(bytes)?;
    // Dropping the stream closes the connection, completing the job.
    Ok(())
}

fn open_raw_connection(port: u16) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match TcpStream::connect((Ipv4Addr::LOCALHOST, port)) {
            Ok(stream) => return stream,
            Err(error) => {
                assert!(
                    Instant::now() < deadline,
                    "the RAW printer never accepted a connection: {error}"
                );
                thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

fn send_raw_job_one_byte_at_a_time(port: u16, bytes: &[u8]) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut stream = loop {
        match TcpStream::connect((Ipv4Addr::LOCALHOST, port)) {
            Ok(stream) => break stream,
            Err(error) => {
                assert!(
                    Instant::now() < deadline,
                    "the RAW printer never accepted data: {error}"
                );
                thread::sleep(Duration::from_millis(25));
            }
        }
    };
    // Disable Nagle and flush each byte so the writes reach the server as
    // separate reads rather than one coalesced buffer.
    stream
        .set_nodelay(true)
        .expect("nodelay should be settable on loopback");
    for byte in bytes {
        stream
            .write_all(&[*byte])
            .expect("each byte should be writable");
        stream.flush().expect("each byte should flush");
        thread::sleep(Duration::from_millis(1));
    }
    // Dropping the stream closes the connection, completing the job.
}

fn wait_for_first_job(web_port: u16) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let metadata = render_metadata(web_port);
        let has_sheets = metadata["sheets"]
            .as_array()
            .is_some_and(|sheets| !sheets.is_empty());
        if has_sheets {
            return metadata;
        }
        assert!(
            Instant::now() < deadline,
            "the captured job did not become visible"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_generation_change(web_port: u16, previous: u64) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let metadata = render_metadata(web_port);
        if metadata["generation"]
            .as_u64()
            .is_some_and(|generation| generation > previous)
        {
            return metadata;
        }
        assert!(
            Instant::now() < deadline,
            "the render generation did not advance"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_render_error(web_port: u16) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let metadata = render_metadata(web_port);
        if metadata["error"].is_string() {
            return metadata;
        }
        assert!(
            Instant::now() < deadline,
            "the render error did not become visible"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn render_metadata(web_port: u16) -> serde_json::Value {
    let response = http_get_bytes(web_port, "/api/render");
    serde_json::from_slice(response_body(&response)).expect("the metadata response should be JSON")
}

fn status_metadata(web_port: u16) -> serde_json::Value {
    let response = http_get_bytes(web_port, "/api/status");
    serde_json::from_slice(response_body(&response)).expect("the status response should be JSON")
}

fn wait_until_receiving(web_port: u16, expected: bool) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let metadata = render_metadata(web_port);
        if metadata["receiving"].as_bool() == Some(expected) {
            return metadata;
        }
        assert!(
            Instant::now() < deadline,
            "the receiving flag did not become {expected}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_until_status_receiving(web_port: u16, expected: bool) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let status = status_metadata(web_port);
        if status["virtual_printer"]["state"].as_str()
            == Some(if expected { "receiving" } else { "ready" })
        {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "the runtime status did not become {}",
            if expected { "receiving" } else { "ready" }
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_until_listening(child: &mut Child, port: u16) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_ok() {
            return;
        }
        if let Some(status) = child
            .try_wait()
            .expect("the child status should be readable")
        {
            panic!("serve command exited early with {status}");
        }
        assert!(
            Instant::now() < deadline,
            "serve command did not listen on port {port}"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

fn http_get_bytes(port: u16, path: &str) -> Vec<u8> {
    // Retry transient loopback failures (see `send_raw_job`) rather than fail on
    // the first refused connection or truncated read.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match http_get_once(port, path) {
            Ok(response) => return response,
            Err(error) => {
                assert!(
                    Instant::now() < deadline,
                    "the web server never answered GET {path}: {error}"
                );
                thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

fn http_get_once(port: u16, path: &str) -> std::io::Result<Vec<u8>> {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    Ok(response)
}

fn open_status_events(port: u16) -> BufReader<TcpStream> {
    let mut stream =
        TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("the web server should accept HTTP");
    write!(
        stream,
        "GET /api/status/events HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .expect("the status event request should be writable");
    BufReader::new(stream)
}

fn next_status_event(events: &mut BufReader<TcpStream>) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut event_name = None;
    let mut data = None;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .unwrap_or_else(|| panic!("the status stream did not produce an event"));
        events
            .get_mut()
            .set_read_timeout(Some(remaining))
            .expect("the status event socket should accept a read timeout");

        let mut line = String::new();
        let read = events
            .read_line(&mut line)
            .unwrap_or_else(|error| panic!("the status stream stalled: {error}"));
        assert!(read != 0, "the status stream closed before its next event");
        let line = line.trim_end_matches(['\r', '\n']);
        if let Some(name) = line.strip_prefix("event: ") {
            event_name = Some(name.to_owned());
        } else if let Some(value) = line.strip_prefix("data: ") {
            data = Some(value.to_owned());
        } else if line.is_empty() {
            if event_name.as_deref() == Some("status") {
                let data = data
                    .as_deref()
                    .expect("a named status event should contain data");
                return serde_json::from_str(data).expect("status event data should be JSON");
            }
            event_name = None;
            data = None;
        }
    }
}

fn response_body(response: &[u8]) -> &[u8] {
    let boundary = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the HTTP response should contain a header boundary");
    &response[boundary + 4..]
}

fn response_header<'a>(response: &'a [u8], requested: &str) -> Option<&'a str> {
    let response = std::str::from_utf8(response).ok()?;
    let head = response.split_once("\r\n\r\n")?.0;
    head.lines().skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name.eq_ignore_ascii_case(requested)).then_some(value.trim())
    })
}

fn stop(child: &mut Child) {
    child.kill().expect("the serve command should be stoppable");
    child.wait().expect("the serve command should be reapable");
}
