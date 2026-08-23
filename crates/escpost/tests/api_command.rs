use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn info_reports_the_version_platform_and_capabilities() {
    let port = unused_loopback_port();
    let mut child = start_api(port);

    wait_until_listening(&mut child, port);
    let response = http_get(port, "/info");
    stop(&mut child);

    assert_eq!(response_status(&response), "HTTP/1.1 200 OK");
    let body: serde_json::Value =
        serde_json::from_slice(response_body(&response)).expect("/info should answer JSON");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["platform"], std::env::consts::OS);
    let capabilities: Vec<&str> = body["capabilities"]
        .as_array()
        .expect("capabilities should be an array")
        .iter()
        .map(|value| value.as_str().expect("each capability is a string"))
        .collect();
    assert!(capabilities.contains(&"usb"));
    assert!(capabilities.contains(&"tcp"));
}

#[test]
fn the_api_binds_loopback_and_not_every_interface() {
    // D1: a LAN-reachable print port is a defect, so this asserts the negative
    // as well as the positive.
    let port = unused_loopback_port();
    let mut child = start_api(port);
    wait_until_listening(&mut child, port);

    let loopback = TcpStream::connect((Ipv4Addr::LOCALHOST, port));
    let routable = non_loopback_address().map(|address| {
        TcpStream::connect_timeout(
            &std::net::SocketAddr::new(address, port),
            Duration::from_millis(300),
        )
    });
    stop(&mut child);

    assert!(loopback.is_ok(), "the API should accept loopback");
    if let Some(result) = routable {
        assert!(
            result.is_err(),
            "the API must not be reachable on a routable address"
        );
    }
}

#[test]
fn a_remote_page_origin_is_refused() {
    let port = unused_loopback_port();
    let mut child = start_api(port);
    wait_until_listening(&mut child, port);

    let response = http_request(
        port,
        "GET",
        "/info",
        &["Origin: https://evil.example".to_owned()],
        &[],
    );
    stop(&mut child);

    assert_eq!(response_status(&response), "HTTP/1.1 403 Forbidden");
    let body: serde_json::Value =
        serde_json::from_slice(response_body(&response)).expect("the refusal should be JSON");
    assert_eq!(body["error"]["code"], "ORIGIN_NOT_GRANTED");
}

#[test]
fn the_extension_and_local_callers_are_accepted() {
    let port = unused_loopback_port();
    let mut child = start_api(port);
    wait_until_listening(&mut child, port);

    let extension = http_request(
        port,
        "GET",
        "/info",
        &["Origin: chrome-extension://cnifebiebidolpmlmgcghpopggfcklmc".to_owned()],
        &[],
    );
    let opaque = http_request(port, "GET", "/info", &["Origin: null".to_owned()], &[]);
    let absent = http_get(port, "/info");
    stop(&mut child);

    assert_eq!(response_status(&extension), "HTTP/1.1 200 OK");
    assert_eq!(response_status(&opaque), "HTTP/1.1 200 OK");
    assert_eq!(response_status(&absent), "HTTP/1.1 200 OK");
}

#[test]
fn printers_reports_identity_transport_and_a_resolvable_profile() {
    let directory = temporary_directory("printers");
    let config = directory.join("printers.toml");
    std::fs::write(
        &config,
        "\
[counter]
transport = \"usb\"
profile = \"TM-T88II\"
vendor_id = \"0x04b8\"
product_id = \"0x0202\"
interface_number = 0
out_endpoint = \"0x01\"

[kitchen]
transport = \"network\"
profile = \"tm-t88\"
host = \"192.0.2.50\"
port = 9100
",
    )
    .expect("the printer configuration should be writable");

    let port = unused_loopback_port();
    let mut child = start_api_with_config(port, &config);
    wait_until_listening(&mut child, port);
    let response = http_get(port, "/printers");
    stop(&mut child);

    assert_eq!(response_status(&response), "HTTP/1.1 200 OK");
    let body: serde_json::Value =
        serde_json::from_slice(response_body(&response)).expect("/printers should answer JSON");
    let printers = body.as_array().expect("/printers returns an array");
    assert_eq!(printers.len(), 2);

    let counter = printers
        .iter()
        .find(|printer| printer["id"] == "counter")
        .expect("the USB printer should be listed");
    assert_eq!(counter["transport"], "usb");
    assert_eq!(counter["profile"], "TM-T88II");
    assert_eq!(counter["device"]["usbVendorId"], 0x04b8);

    let kitchen = printers
        .iter()
        .find(|printer| printer["id"] == "kitchen")
        .expect("the TCP printer should be listed");
    assert_eq!(kitchen["transport"], "tcp");
    // "tm-t88" is in no catalog. Reporting it as a real profile is the bug.
    assert_eq!(kitchen["profile"], serde_json::Value::Null);
    assert_eq!(kitchen["device"]["host"], "192.0.2.50");
    assert_eq!(kitchen["device"]["port"], 9100);

    std::fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[test]
fn info_advertises_device_identity_so_a_client_can_detect_an_older_daemon() {
    let port = unused_loopback_port();
    let mut child = start_api(port);
    wait_until_listening(&mut child, port);
    let response = http_get(port, "/info");
    stop(&mut child);

    let body: serde_json::Value =
        serde_json::from_slice(response_body(&response)).expect("/info should answer JSON");
    let capabilities: Vec<&str> = body["capabilities"]
        .as_array()
        .expect("capabilities should be an array")
        .iter()
        .map(|value| value.as_str().expect("each capability is a string"))
        .collect();
    assert!(capabilities.contains(&"device-identity"));
}

fn start_api_with_config(port: u16, config: &std::path::Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "api",
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--config",
            config.to_str().expect("the config path should be UTF-8"),
            "--non-interactive",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the escpost command should start")
}

fn temporary_directory(case: &str) -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the system clock should be after the Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "escpost-api-{case}-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir(&path).expect("the test directory should be creatable");
    path
}

#[test]
fn the_default_printer_is_the_first_one_listed() {
    let directory = temporary_directory("default");
    let config = directory.join("printers.toml");
    std::fs::write(
        &config,
        "\
[zulu]
transport = \"network\"
host = \"192.0.2.50\"
port = 9100

[alpha]
transport = \"network\"
host = \"192.0.2.51\"
port = 9100
",
    )
    .expect("the printer configuration should be writable");

    let port = unused_loopback_port();
    let mut child = start_api_with_config(port, &config);
    wait_until_listening(&mut child, port);
    let listed = http_get(port, "/printers");
    let default = http_get(port, "/printers/default");
    stop(&mut child);

    let listed: serde_json::Value =
        serde_json::from_slice(response_body(&listed)).expect("/printers should answer JSON");
    let default_body: serde_json::Value =
        serde_json::from_slice(response_body(&default)).expect("the default should answer JSON");

    assert_eq!(response_status(&default), "HTTP/1.1 200 OK");
    assert_eq!(default_body["id"], listed[0]["id"]);
    // Both are unavailable, so the tie breaks on name: alpha before zulu.
    assert_eq!(default_body["id"], "alpha");

    std::fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[test]
fn there_is_no_default_printer_when_none_is_configured() {
    let directory = temporary_directory("no-default");
    let config = directory.join("printers.toml");
    std::fs::write(&config, "").expect("an empty configuration should be writable");

    let port = unused_loopback_port();
    let mut child = start_api_with_config(port, &config);
    wait_until_listening(&mut child, port);
    let response = http_get(port, "/printers/default");
    stop(&mut child);

    assert_eq!(response_status(&response), "HTTP/1.1 404 Not Found");
    let body: serde_json::Value =
        serde_json::from_slice(response_body(&response)).expect("the 404 should be JSON");
    // The extension turns exactly this code into `null` rather than an error.
    assert_eq!(body["error"]["code"], "PRINTER_NOT_FOUND");

    std::fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[test]
fn a_json_print_reaches_the_printer_with_its_bytes_unchanged() {
    let directory = temporary_directory("json-print");
    let config = directory.join("printers.toml");
    let (printer, printer_port) = fake_printer();
    std::fs::write(
        &config,
        format!(
            "\
[counter]
transport = \"network\"
host = \"127.0.0.1\"
port = {printer_port}
"
        ),
    )
    .expect("the printer configuration should be writable");

    let received = thread::spawn(move || {
        let (mut connection, _) = printer.accept().expect("the printer should be reached");
        let mut bytes = Vec::new();
        connection
            .read_to_end(&mut bytes)
            .expect("the print connection should close cleanly");
        bytes
    });

    let port = unused_loopback_port();
    let mut child = start_api_with_config(port, &config);
    wait_until_listening(&mut child, port);
    let response = http_request(
        port,
        "POST",
        "/print",
        &["Content-Type: application/json".to_owned()],
        br#"{"printer":"counter","data":"G0BIaQo="}"#,
    );
    stop(&mut child);

    assert_eq!(response_status(&response), "HTTP/1.1 200 OK");
    let body: serde_json::Value =
        serde_json::from_slice(response_body(&response)).expect("the print should answer JSON");
    assert!(
        body["job_id"].as_str().is_some_and(|id| !id.is_empty()),
        "a print should be given a job id"
    );
    assert_eq!(
        received.join().expect("the printer thread should finish"),
        vec![0x1b, 0x40, b'H', b'i', b'\n']
    );

    std::fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[test]
fn printing_to_an_unconfigured_printer_is_a_typed_404() {
    let directory = temporary_directory("unknown-printer");
    let config = directory.join("printers.toml");
    std::fs::write(&config, "").expect("an empty configuration should be writable");

    let port = unused_loopback_port();
    let mut child = start_api_with_config(port, &config);
    wait_until_listening(&mut child, port);
    let response = http_request(
        port,
        "POST",
        "/print",
        &["Content-Type: application/json".to_owned()],
        br#"{"printer":"nope","data":""}"#,
    );
    stop(&mut child);

    assert_eq!(response_status(&response), "HTTP/1.1 404 Not Found");
    let body: serde_json::Value =
        serde_json::from_slice(response_body(&response)).expect("the refusal should be JSON");
    assert_eq!(body["error"]["code"], "PRINTER_NOT_FOUND");

    std::fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[test]
fn an_octet_stream_print_sends_the_bytes_with_no_re_encoding() {
    let directory = temporary_directory("octet-print");
    let config = directory.join("printers.toml");
    let (printer, printer_port) = fake_printer();
    std::fs::write(
        &config,
        format!(
            "\
[counter]
transport = \"network\"
host = \"127.0.0.1\"
port = {printer_port}
"
        ),
    )
    .expect("the printer configuration should be writable");

    let received = thread::spawn(move || {
        let (mut connection, _) = printer.accept().expect("the printer should be reached");
        let mut bytes = Vec::new();
        connection
            .read_to_end(&mut bytes)
            .expect("the print connection should close cleanly");
        bytes
    });

    // Deliberately not valid UTF-8 and not valid base64: this path must carry
    // arbitrary bytes, which is the whole reason it exists.
    let payload: Vec<u8> = vec![0x1b, 0x40, 0x00, 0xff, 0xfe, 0x0a];

    let port = unused_loopback_port();
    let mut child = start_api_with_config(port, &config);
    wait_until_listening(&mut child, port);
    let response = http_request(
        port,
        "POST",
        "/print?printer=counter",
        &["Content-Type: application/octet-stream".to_owned()],
        &payload,
    );
    stop(&mut child);

    assert_eq!(response_status(&response), "HTTP/1.1 200 OK");
    assert_eq!(
        received.join().expect("the printer thread should finish"),
        payload
    );

    std::fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[test]
fn an_octet_stream_print_without_a_printer_parameter_says_so() {
    let port = unused_loopback_port();
    let mut child = start_api(port);
    wait_until_listening(&mut child, port);
    let response = http_request(
        port,
        "POST",
        "/print",
        &["Content-Type: application/octet-stream".to_owned()],
        &[0x1b, 0x40],
    );
    stop(&mut child);

    assert_eq!(response_status(&response), "HTTP/1.1 400 Bad Request");
    let body: serde_json::Value =
        serde_json::from_slice(response_body(&response)).expect("the refusal should be JSON");
    assert_eq!(body["error"]["code"], "PRINTER_NOT_FOUND");
}

#[test]
fn two_concurrent_prints_to_one_printer_are_serialised() {
    // A RAW TCP printer is single-session: the second connection is refused
    // while the first is open. Without serialisation one of two simultaneous
    // prints fails, and which one is a race.
    let directory = temporary_directory("concurrent");
    let config = directory.join("printers.toml");
    let (printer, printer_port) = fake_printer();
    std::fs::write(
        &config,
        format!(
            "\
[counter]
transport = \"network\"
host = \"127.0.0.1\"
port = {printer_port}
"
        ),
    )
    .expect("the printer configuration should be writable");

    // Accept one connection at a time, holding each briefly, so overlapping
    // prints would be visible as a refusal or interleaved bytes.
    let receiver = thread::spawn(move || {
        let mut jobs = Vec::new();
        for _ in 0..2 {
            let (mut connection, _) = printer.accept().expect("each print should arrive");
            let mut bytes = Vec::new();
            connection
                .read_to_end(&mut bytes)
                .expect("each print should close cleanly");
            thread::sleep(Duration::from_millis(120));
            jobs.push(bytes);
        }
        jobs
    });

    let port = unused_loopback_port();
    let mut child = start_api_with_config(port, &config);
    wait_until_listening(&mut child, port);

    let first = thread::spawn(move || {
        http_request(
            port,
            "POST",
            "/print",
            &["Content-Type: application/json".to_owned()],
            br#"{"printer":"counter","data":"QUFB"}"#,
        )
    });
    let second = thread::spawn(move || {
        http_request(
            port,
            "POST",
            "/print",
            &["Content-Type: application/json".to_owned()],
            br#"{"printer":"counter","data":"QkJC"}"#,
        )
    });
    let first = first.join().expect("the first print should finish");
    let second = second.join().expect("the second print should finish");
    stop(&mut child);

    assert_eq!(response_status(&first), "HTTP/1.1 200 OK");
    assert_eq!(response_status(&second), "HTTP/1.1 200 OK");

    let mut jobs = receiver.join().expect("the printer thread should finish");
    jobs.sort();
    // Each job arrived whole. Interleaving would show as a job that is neither.
    assert_eq!(jobs, vec![b"AAA".to_vec(), b"BBB".to_vec()]);

    std::fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[test]
fn pinning_an_extension_id_refuses_every_other_extension() {
    let pinned = "cnifebiebidolpmlmgcghpopggfcklmc";
    let port = unused_loopback_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "api",
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--extension-id",
            pinned,
            "--non-interactive",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the escpost command should start");
    wait_until_listening(&mut child, port);

    let allowed = http_request(
        port,
        "GET",
        "/info",
        &[format!("Origin: chrome-extension://{pinned}")],
        &[],
    );
    let refused = http_request(
        port,
        "GET",
        "/info",
        &["Origin: chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned()],
        &[],
    );
    // Pinning narrows which extension may call, never whether a local backend
    // may: L1–L4 do not require an extension to exist at all.
    let local = http_get(port, "/info");
    stop(&mut child);

    assert_eq!(response_status(&allowed), "HTTP/1.1 200 OK");
    assert_eq!(response_status(&refused), "HTTP/1.1 403 Forbidden");
    assert_eq!(response_status(&local), "HTTP/1.1 200 OK");
}

#[test]
fn the_api_makes_no_outbound_connection_while_serving() {
    // D4. escpost is Apache-2.0 and must stay independently useful: no licence
    // check, no telemetry, no account. A local listener stands in for
    // "somewhere else" — if the API ever phones home, the most likely first
    // version of that bug is a connection this catches.
    //
    // The watcher is on a port nothing is configured to use, which is the
    // distinction that matters: `/printers` legitimately connects to every
    // configured network printer to report its availability, so a watcher
    // doubling as a configured printer would prove nothing.
    let (watcher, _watcher_port) = fake_printer();
    watcher
        .set_nonblocking(true)
        .expect("the watcher should be non-blocking");

    let directory = temporary_directory("no-egress");
    let config = directory.join("printers.toml");
    std::fs::write(&config, "").expect("an empty configuration should be writable");

    let port = unused_loopback_port();
    let mut child = start_api_with_config(port, &config);
    wait_until_listening(&mut child, port);

    let _ = http_get(port, "/info");
    let _ = http_get(port, "/printers");
    let _ = http_get(port, "/printers/default");
    let _ = http_request(
        port,
        "POST",
        "/print",
        &["Content-Type: application/json".to_owned()],
        br#"{"printer":"nope","data":""}"#,
    );
    thread::sleep(Duration::from_millis(400));
    stop(&mut child);

    assert!(
        watcher.accept().is_err(),
        "serving the API must open no connection of its own"
    );

    std::fs::remove_dir_all(directory).expect("the test directory should be removable");
}

/// A routable IPv4 address of this machine, if it has one. Returns None on a
/// host with only loopback, where the negative assertion cannot be made.
fn non_loopback_address() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    // Connecting a UDP socket sends nothing; it only selects a source address.
    socket.connect(("192.0.2.1", 9)).ok()?;
    let address = socket.local_addr().ok()?.ip();
    (!address.is_loopback()).then_some(address)
}

fn start_api(port: u16) -> Child {
    Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "api",
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--non-interactive",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the escpost command should start")
}

static ISSUED: Mutex<Option<HashSet<u16>>> = Mutex::new(None);

/// Claim `port` in the shared registry, returning false if it was already
/// handed out.
fn claim(port: u16) -> bool {
    let mut issued = ISSUED.lock().expect("the issued-port set should be usable");
    issued.get_or_insert_with(HashSet::new).insert(port)
}

/// A loopback port no other test in this binary has been given.
///
/// The listener is dropped before returning, so there is a window where the
/// port is free but spoken for. `fake_printer` claims from the same registry
/// precisely so it cannot bind into that window and knock over a server that
/// was about to.
fn unused_loopback_port() -> u16 {
    let mut held = Vec::new();
    for _ in 0..64 {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .expect("an ephemeral loopback port should be available");
        let port = listener
            .local_addr()
            .expect("the listener should have a local address")
            .port();
        if claim(port) {
            return port;
        }
        held.push(listener);
    }
    panic!("no unissued ephemeral loopback port was available after 64 attempts");
}

/// A bound listener standing in for a RAW TCP printer, on a port that is
/// registered so no API server is later told to use it.
fn fake_printer() -> (TcpListener, u16) {
    let mut held = Vec::new();
    for _ in 0..64 {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .expect("a fake printer should bind");
        let port = listener
            .local_addr()
            .expect("the fake printer has an address")
            .port();
        if claim(port) {
            return (listener, port);
        }
        held.push(listener);
    }
    panic!("no unissued ephemeral loopback port was available for a fake printer");
}

fn wait_until_listening(child: &mut Child, port: u16) {
    let deadline = Instant::now() + Duration::from_secs(10);
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
            "api command did not listen on port {port}"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

fn stop(child: &mut Child) {
    child.kill().expect("the api command should be stoppable");
    child.wait().expect("the api command should be reapable");
}

fn http_get(port: u16, path: &str) -> Vec<u8> {
    http_request(port, "GET", path, &[], &[])
}

/// One raw HTTP/1.1 request. `headers` are extra lines without CRLF; `body` is
/// sent verbatim with a matching Content-Length.
fn http_request(port: u16, method: &str, path: &str, headers: &[String], body: &[u8]) -> Vec<u8> {
    let mut stream =
        TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("the API should accept HTTP");
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

fn response_head(response: &[u8]) -> &str {
    let boundary = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the HTTP response should contain a header boundary");
    std::str::from_utf8(&response[..boundary]).expect("HTTP headers should be UTF-8")
}

fn response_status(response: &[u8]) -> &str {
    response_head(response)
        .lines()
        .next()
        .expect("HTTP response should have a status line")
}

fn response_body(response: &[u8]) -> &[u8] {
    let boundary = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the HTTP response should contain a header boundary");
    &response[boundary + 4..]
}
