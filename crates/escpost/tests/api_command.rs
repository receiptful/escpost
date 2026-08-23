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

fn unused_loopback_port() -> u16 {
    static ISSUED: Mutex<Option<HashSet<u16>>> = Mutex::new(None);
    let mut issued = ISSUED.lock().expect("the issued-port set should be usable");
    let issued = issued.get_or_insert_with(HashSet::new);
    let mut held = Vec::new();
    for _ in 0..64 {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .expect("an ephemeral loopback port should be available");
        let port = listener
            .local_addr()
            .expect("the listener should have a local address")
            .port();
        if issued.insert(port) {
            return port;
        }
        held.push(listener);
    }
    panic!("no unissued ephemeral loopback port was available after 64 attempts");
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
