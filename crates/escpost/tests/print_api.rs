use std::collections::HashSet;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[test]
fn octet_stream_reaches_the_printer_without_reencoding() {
    // Defect caught: the HTTP adapter decodes, recodes, or otherwise changes
    // bytes before passing them to the shared printing operation.
    let directory = TemporaryDirectory::new("exact-bytes");
    let (printer, printer_port) = fake_printer();
    let config = network_configuration(&directory, printer_port);
    let api_port = unused_loopback_port();
    let mut api = start_api(api_port, &config, None);
    wait_until_listening(&mut api.child, api_port);
    let received = receive_jobs(printer, 1);

    let payload = vec![0x1b, 0x40, 0x00, 0xff, 0xfe, 0x0a];
    let response = http_request(
        api_port,
        "POST",
        "/api/print?printer=counter",
        &["Content-Type: application/octet-stream".to_owned()],
        &payload,
    );

    assert_eq!(response_status(&response), "HTTP/1.1 200 OK");
    assert_eq!(
        response_header(&response, "cache-control"),
        Some("no-store")
    );
    let body: serde_json::Value = serde_json::from_slice(response_body(&response))
        .expect("a successful print response should be JSON");
    assert_eq!(body["job_id"], "job-0");
    assert_eq!(
        received.join().expect("the printer should finish"),
        vec![payload]
    );
}

#[test]
fn absent_origins_are_accepted_as_local_program_calls() {
    // Defect caught: an origin allow-list accidentally rejects curl, local
    // programs, and other callers that legitimately omit Origin.
    let directory = TemporaryDirectory::new("absent-origin");
    let config = empty_configuration(&directory);
    let api_port = unused_loopback_port();
    let mut api = start_api(api_port, &config, None);
    wait_until_listening(&mut api.child, api_port);

    let response = raw_print(api_port, "missing", &[], b"local");

    assert_typed_error(&response, "HTTP/1.1 404 Not Found", "PRINTER_NOT_FOUND");
}

#[test]
fn exact_extension_scheme_origins_are_accepted() {
    // Defect caught: one supported browser engine is excluded, or the guard
    // accepts only ordinary local calls and blocks the extension bridge.
    let directory = TemporaryDirectory::new("extension-origins");
    let config = empty_configuration(&directory);
    let api_port = unused_loopback_port();
    let mut api = start_api(api_port, &config, None);
    wait_until_listening(&mut api.child, api_port);

    for origin in [
        "chrome-extension://abcdefghijklmnop",
        "moz-extension://a1b2c3d4-0000-4000-8000-000000000000",
        "safari-web-extension://A1B2C3D4-0000-4000-8000-000000000000",
    ] {
        let response = raw_print(api_port, "missing", &[format!("Origin: {origin}")], b"job");
        assert_typed_error(&response, "HTTP/1.1 404 Not Found", "PRINTER_NOT_FOUND");
    }
}

#[test]
fn pinning_an_extension_id_accepts_only_that_exact_id() {
    // Defect caught: --extension-id is ignored, compared by prefix, or blocks
    // origin-less local programs along with other extensions.
    let directory = TemporaryDirectory::new("pinned-origin");
    let config = empty_configuration(&directory);
    let api_port = unused_loopback_port();
    let pinned = "abcdefghijklmnop";
    let mut api = start_api(api_port, &config, Some(pinned));
    wait_until_listening(&mut api.child, api_port);

    let allowed = raw_print(
        api_port,
        "missing",
        &[format!("Origin: chrome-extension://{pinned}")],
        b"job",
    );
    let refused = raw_print(
        api_port,
        "missing",
        &["Origin: chrome-extension://abcdefghijklmnop-lookalike".to_owned()],
        b"job",
    );
    let local = raw_print(api_port, "missing", &[], b"job");

    assert_typed_error(&allowed, "HTTP/1.1 404 Not Found", "PRINTER_NOT_FOUND");
    assert_typed_error(&refused, "HTTP/1.1 403 Forbidden", "ORIGIN_NOT_GRANTED");
    assert_typed_error(&local, "HTTP/1.1 404 Not Found", "PRINTER_NOT_FOUND");
}

#[test]
fn null_and_lookalike_origins_are_rejected() {
    // Defect caught: opaque sandboxed documents, web pages, paths, and schemes
    // that merely resemble extension origins pass the negative filter.
    let directory = TemporaryDirectory::new("rejected-origins");
    let config = empty_configuration(&directory);
    let api_port = unused_loopback_port();
    let mut api = start_api(api_port, &config, None);
    wait_until_listening(&mut api.child, api_port);

    for origin in [
        "null",
        "https://example.invalid",
        "http://localhost:5173",
        "file://",
        "web-extension://abcdefghijklmnop",
        "https://example.invalid/chrome-extension://abcdefghijklmnop",
        "chrome-extension://",
        "chrome-extension://abcdefghijklmnop/path",
    ] {
        let response = raw_print(api_port, "missing", &[format!("Origin: {origin}")], b"job");
        assert_typed_error(&response, "HTTP/1.1 403 Forbidden", "ORIGIN_NOT_GRANTED");
    }
}

#[test]
fn ordinary_web_origins_are_rejected_before_printer_io() {
    // Defect caught: origin checking happens after printer resolution or the
    // outbound connection, allowing a normal web page to cause side effects.
    let directory = TemporaryDirectory::new("origin-before-io");
    let (printer, printer_port) = fake_printer();
    printer
        .set_nonblocking(true)
        .expect("the fake printer should become non-blocking");
    let config = network_configuration(&directory, printer_port);
    let api_port = unused_loopback_port();
    let mut api = start_api(api_port, &config, None);
    wait_until_listening(&mut api.child, api_port);

    let response = http_request(
        api_port,
        "POST",
        "/api/print?printer=counter",
        &[
            "Content-Type: application/octet-stream".to_owned(),
            "Origin: https://example.invalid".to_owned(),
        ],
        b"never print this",
    );

    assert_typed_error(&response, "HTTP/1.1 403 Forbidden", "ORIGIN_NOT_GRANTED");
    thread::sleep(Duration::from_millis(100));
    assert!(
        matches!(printer.accept(), Err(error) if error.kind() == ErrorKind::WouldBlock),
        "a rejected origin must not reach the configured printer"
    );
}

#[test]
fn an_unknown_printer_has_the_public_not_found_code() {
    // Defect caught: the intermediate Task 1 error vocabulary leaks into the
    // live contract consumed by the browser SDK.
    let directory = TemporaryDirectory::new("unknown-printer");
    let config = empty_configuration(&directory);
    let api_port = unused_loopback_port();
    let mut api = start_api(api_port, &config, None);
    wait_until_listening(&mut api.child, api_port);

    let response = raw_print(api_port, "missing", &[], b"job");

    assert_typed_error(&response, "HTTP/1.1 404 Not Found", "PRINTER_NOT_FOUND");
}

#[test]
fn unsupported_or_missing_content_types_are_rejected() {
    // Defect caught: the route silently accepts text, JSON/base64, or a missing
    // media type as a second raw-print contract.
    let directory = TemporaryDirectory::new("content-type");
    let config = empty_configuration(&directory);
    let api_port = unused_loopback_port();
    let mut api = start_api(api_port, &config, None);
    wait_until_listening(&mut api.child, api_port);

    for headers in [
        vec!["Content-Type: text/plain".to_owned()],
        vec!["Content-Type: application/json".to_owned()],
        Vec::new(),
    ] {
        let response = http_request(
            api_port,
            "POST",
            "/api/print?printer=missing",
            &headers,
            b"job",
        );
        assert_typed_error(
            &response,
            "HTTP/1.1 415 Unsupported Media Type",
            "UNSUPPORTED_MEDIA_TYPE",
        );
    }
}

#[test]
fn blank_and_whitespace_printer_queries_are_bad_requests() {
    // Defect caught: empty or whitespace-only configured names cross the live
    // query boundary and are reported as unknown printers instead of missing.
    let directory = TemporaryDirectory::new("blank-printer");
    let config = empty_configuration(&directory);
    let api_port = unused_loopback_port();
    let mut api = start_api(api_port, &config, None);
    wait_until_listening(&mut api.child, api_port);

    for path in ["/api/print?printer=", "/api/print?printer=%20%20%20"] {
        let response = http_request(
            api_port,
            "POST",
            path,
            &["Content-Type: application/octet-stream".to_owned()],
            b"job",
        );
        assert_typed_error(&response, "HTTP/1.1 400 Bad Request", "PRINTER_REQUIRED");
    }
}

#[test]
fn configuration_failures_are_not_misreported_as_unknown_printers() {
    // Defect caught: every resolver error is flattened to PRINTER_NOT_FOUND,
    // hiding invalid or unreadable printer configuration.
    let directory = TemporaryDirectory::new("invalid-config");
    let config = directory.path.join("printers.toml");
    std::fs::write(&config, "this is not = valid = toml")
        .expect("the invalid configuration fixture should be writable");
    let api_port = unused_loopback_port();
    let mut api = start_api(api_port, &config, None);
    wait_until_listening(&mut api.child, api_port);

    let response = raw_print(api_port, "counter", &[], b"job");

    assert_typed_error(
        &response,
        "HTTP/1.1 500 Internal Server Error",
        "PRINT_FAILED",
    );
}

#[test]
fn configured_path_is_shared_by_inventory_stream_and_printing() {
    // Defect caught: --config reaches only one of list, list/events, and print,
    // so the same server exposes incompatible configured-printer namespaces.
    let directory = TemporaryDirectory::new("shared-config");
    let unavailable_printer_port = unused_loopback_port();
    let config = network_configuration(&directory, unavailable_printer_port);
    let api_port = unused_loopback_port();
    let mut api = start_api(api_port, &config, None);
    wait_until_listening(&mut api.child, api_port);

    let listing = http_request(api_port, "GET", "/api/printers/list", &[], b"");
    assert_eq!(response_status(&listing), "HTTP/1.1 200 OK");
    let listing: serde_json::Value =
        serde_json::from_slice(response_body(&listing)).expect("the printer list should be JSON");
    assert_eq!(listing["printers"][0]["name"], "counter");

    let event = http_stream_until(
        api_port,
        "/api/printers/list/events",
        b"\"name\":\"counter\"",
    );
    assert!(
        event
            .windows(b"\"name\":\"counter\"".len())
            .any(|window| window == b"\"name\":\"counter\""),
        "the inventory event should use the explicit configuration: {}",
        String::from_utf8_lossy(&event)
    );

    let print = raw_print(api_port, "counter", &[], b"job");
    assert_typed_error(&print, "HTTP/1.1 500 Internal Server Error", "PRINT_FAILED");
}

#[test]
fn two_concurrent_jobs_to_one_printer_arrive_whole_and_serialized() {
    // Defect caught: two handlers open the same physical target concurrently.
    // Large bodies back-pressure the first connection while the fake printer
    // checks that a second connection has not arrived yet.
    let directory = TemporaryDirectory::new("serialized");
    let (printer, printer_port) = fake_printer();
    let config = network_configuration(&directory, printer_port);
    let api_port = unused_loopback_port();
    let mut api = start_api(api_port, &config, None);
    wait_until_listening(&mut api.child, api_port);

    let first_payload = vec![b'A'; 8 * 1024 * 1024];
    let second_payload = vec![b'B'; 8 * 1024 * 1024];
    let first_expected = first_payload.clone();
    let second_expected = second_payload.clone();
    let received = thread::spawn(move || receive_serialized_pair(printer));

    let first = thread::spawn(move || raw_print(api_port, "counter", &[], &first_payload));
    let second = thread::spawn(move || raw_print(api_port, "counter", &[], &second_payload));
    let first = first.join().expect("the first HTTP request should finish");
    let second = second
        .join()
        .expect("the second HTTP request should finish");
    let (overlapped, mut jobs) = received.join().expect("the fake printer should finish");

    assert_eq!(response_status(&first), "HTTP/1.1 200 OK");
    assert_eq!(response_status(&second), "HTTP/1.1 200 OK");
    assert!(
        !overlapped,
        "the second connection arrived before the first completed"
    );
    jobs.sort();
    assert_eq!(jobs, vec![first_expected, second_expected]);
}

#[test]
fn bodies_larger_than_eight_mebibytes_use_a_no_store_api_failure() {
    // Defect caught: the route inherits an implicit framework limit, accepts an
    // unbounded body, or returns Axum's cacheable plain-text rejection.
    let directory = TemporaryDirectory::new("body-limit");
    let config = empty_configuration(&directory);
    let api_port = unused_loopback_port();
    let mut api = start_api(api_port, &config, None);
    wait_until_listening(&mut api.child, api_port);

    let response = raw_print(api_port, "missing", &[], &vec![0u8; 8 * 1024 * 1024 + 1]);

    assert_typed_error(
        &response,
        "HTTP/1.1 413 Payload Too Large",
        "PAYLOAD_TOO_LARGE",
    );
}

#[test]
fn malformed_query_rejections_use_the_no_store_api_envelope() {
    // Defect caught: Query extractor failures bypass ApiFailure and return the
    // framework's default cacheable plain-text response.
    let directory = TemporaryDirectory::new("bad-query");
    let config = empty_configuration(&directory);
    let api_port = unused_loopback_port();
    let mut api = start_api(api_port, &config, None);
    wait_until_listening(&mut api.child, api_port);

    let response = http_request(
        api_port,
        "POST",
        "/api/print?printer=first&printer=second",
        &["Content-Type: application/octet-stream".to_owned()],
        b"job",
    );

    assert_typed_error(&response, "HTTP/1.1 400 Bad Request", "INVALID_REQUEST");
}

fn raw_print(port: u16, printer: &str, headers: &[String], body: &[u8]) -> Vec<u8> {
    let mut headers = headers.to_vec();
    headers.push("Content-Type: application/octet-stream".to_owned());
    http_request(
        port,
        "POST",
        &format!("/api/print?printer={printer}"),
        &headers,
        body,
    )
}

fn assert_typed_error(response: &[u8], status: &str, code: &str) {
    assert_eq!(response_status(response), status);
    assert_eq!(response_header(response, "cache-control"), Some("no-store"));
    let body: serde_json::Value = serde_json::from_slice(response_body(response))
        .expect("the failure should use the JSON API envelope");
    assert_eq!(body["error"]["code"], code);
}

struct RunningApi {
    child: Child,
}

impl Drop for RunningApi {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_api(port: u16, config: &Path, extension_id: Option<&str>) -> RunningApi {
    let mut command = Command::new(env!("CARGO_BIN_EXE_escpost"));
    command.args([
        "serve",
        "--web-listen",
        &format!("127.0.0.1:{port}"),
        "--no-open",
        "--no-web-app",
        "--config",
        config.to_str().expect("the config path should be UTF-8"),
        "--non-interactive",
    ]);
    if let Some(extension_id) = extension_id {
        command.args(["--extension-id", extension_id]);
    }
    RunningApi {
        child: command
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the escpost command should start"),
    }
}

fn wait_until_listening(child: &mut Child, port: u16) {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    loop {
        if TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_ok() {
            return;
        }
        if let Some(status) = child
            .try_wait()
            .expect("the child status should be readable")
        {
            let mut stderr = String::new();
            child
                .stderr
                .take()
                .expect("stderr should be piped")
                .read_to_string(&mut stderr)
                .expect("stderr should be readable");
            panic!("api command exited early with {status}:\n{stderr}");
        }
        assert!(
            Instant::now() < deadline,
            "the API did not listen on {port}"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

static ISSUED_PORTS: Mutex<Option<HashSet<u16>>> = Mutex::new(None);

fn claim_port(port: u16) -> bool {
    ISSUED_PORTS
        .lock()
        .expect("the issued-port registry should be usable")
        .get_or_insert_with(HashSet::new)
        .insert(port)
}

fn unused_loopback_port() -> u16 {
    let mut held = Vec::new();
    for _ in 0..64 {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .expect("an ephemeral loopback port should be available");
        let port = listener
            .local_addr()
            .expect("the listener should have an address")
            .port();
        if claim_port(port) {
            return port;
        }
        held.push(listener);
    }
    panic!("no unissued loopback port was available");
}

fn fake_printer() -> (TcpListener, u16) {
    let mut held = Vec::new();
    for _ in 0..64 {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .expect("a fake printer should bind");
        let port = listener
            .local_addr()
            .expect("the fake printer should have an address")
            .port();
        if claim_port(port) {
            return (listener, port);
        }
        held.push(listener);
    }
    panic!("no unissued loopback port was available for a fake printer");
}

fn receive_jobs(listener: TcpListener, count: usize) -> thread::JoinHandle<Vec<Vec<u8>>> {
    thread::spawn(move || {
        (0..count)
            .map(|_| {
                let mut connection = accept_before_deadline(&listener);
                let mut bytes = Vec::new();
                connection
                    .read_to_end(&mut bytes)
                    .expect("the print connection should close cleanly");
                bytes
            })
            .collect()
    })
}

fn accept_before_deadline(listener: &TcpListener) -> TcpStream {
    listener
        .set_nonblocking(true)
        .expect("the fake printer should become non-blocking");
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    loop {
        match listener.accept() {
            Ok((connection, _)) => {
                connection
                    .set_nonblocking(false)
                    .expect("the print connection should become blocking");
                connection
                    .set_read_timeout(Some(REQUEST_TIMEOUT))
                    .expect("the print connection should have a deadline");
                return connection;
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "the printer was never reached");
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("the fake printer should accept: {error}"),
        }
    }
}

fn receive_serialized_pair(listener: TcpListener) -> (bool, Vec<Vec<u8>>) {
    let mut first = accept_before_deadline(&listener);
    listener
        .set_nonblocking(true)
        .expect("the fake printer should be non-blocking");
    thread::sleep(Duration::from_millis(200));
    let early_second = match listener.accept() {
        Ok((connection, _)) => Some(connection),
        Err(error) if error.kind() == ErrorKind::WouldBlock => None,
        Err(error) => panic!("the fake printer should inspect its queue: {error}"),
    };

    let mut first_bytes = Vec::new();
    first
        .read_to_end(&mut first_bytes)
        .expect("the first print should close cleanly");
    let overlapped = early_second.is_some();
    let mut second = early_second.unwrap_or_else(|| accept_before_deadline(&listener));
    second
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .expect("the second print should have a deadline");
    let mut second_bytes = Vec::new();
    second
        .read_to_end(&mut second_bytes)
        .expect("the second print should close cleanly");
    (overlapped, vec![first_bytes, second_bytes])
}

fn http_request(port: u16, method: &str, path: &str, headers: &[String], body: &[u8]) -> Vec<u8> {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .expect("the API should accept the HTTP connection");
    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .expect("the HTTP connection should have a read deadline");
    stream
        .set_write_timeout(Some(REQUEST_TIMEOUT))
        .expect("the HTTP connection should have a write deadline");
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for header in headers {
        request.push_str(header);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .expect("the HTTP request head should be writable");
    stream
        .write_all(body)
        .expect("the HTTP request body should be writable");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .expect("the HTTP response should be readable");
    response
}

fn http_stream_until(port: u16, path: &str, needle: &[u8]) -> Vec<u8> {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .expect("the API should accept the event stream");
    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .expect("the event stream should have a read deadline");
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: text/event-stream\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .expect("the event stream request should be writable");
    let mut response = Vec::new();
    let mut chunk = [0u8; 1024];
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    while !response
        .windows(needle.len())
        .any(|window| window == needle)
    {
        assert!(
            Instant::now() < deadline,
            "the expected event never arrived"
        );
        let read = stream
            .read(&mut chunk)
            .expect("the event stream should be readable");
        assert!(
            read != 0,
            "the event stream closed before its first snapshot"
        );
        response.extend_from_slice(&chunk[..read]);
    }
    response
}

fn response_head(response: &[u8]) -> &str {
    let boundary = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the HTTP response should have a header boundary");
    std::str::from_utf8(&response[..boundary]).expect("HTTP headers should be UTF-8")
}

fn response_status(response: &[u8]) -> &str {
    response_head(response)
        .lines()
        .next()
        .expect("the HTTP response should have a status line")
}

fn response_header<'a>(response: &'a [u8], name: &str) -> Option<&'a str> {
    response_head(response).lines().skip(1).find_map(|line| {
        let (header, value) = line.split_once(':')?;
        header.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

fn response_body(response: &[u8]) -> &[u8] {
    let boundary = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the HTTP response should have a header boundary");
    &response[boundary + 4..]
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
            "escpost-print-api-{case}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("the temporary directory should be creatable");
        Self { path }
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn empty_configuration(directory: &TemporaryDirectory) -> PathBuf {
    let path = directory.path.join("printers.toml");
    std::fs::write(&path, "").expect("the empty configuration should be writable");
    path
}

fn network_configuration(directory: &TemporaryDirectory, printer_port: u16) -> PathBuf {
    let path = directory.path.join("printers.toml");
    std::fs::write(
        &path,
        format!(
            "[counter]\ntransport = \"network\"\nhost = \"127.0.0.1\"\nport = {printer_port}\n"
        ),
    )
    .expect("the network printer configuration should be writable");
    path
}
