use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[test]
fn web_mode_serves_the_embedded_workbench() {
    let port = unused_loopback_port();
    let mut child = start_case_web("single-sheet", port);

    wait_until_listening(&mut child, port);
    let response = http_get(port, "/");
    stop(&mut child);

    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.contains("<title>ESCPost render</title>"));
    assert!(response.contains("<div id=\"sheets\""));
    // Anti-aliased renders are smoothed; the faithful dot grid stays pixelated.
    assert!(response.contains("#sheets.antialiased"));
    assert!(response.contains("id=\"margin\""));
    assert!(response.contains("Paper margin"));
    assert!(response.contains("id=\"connection\""));
    assert!(response.contains("id=\"completion\""));
    assert!(response.contains("id=\"jobStatus\""));
    assert!(response.contains("id=\"receiving\""));
    assert!(response.contains("id=\"download\""));
    assert!(response.contains("id=\"warnings\""));
    assert!(response.contains("id=\"magnifyHint\""));
    assert!(response.contains("id=\"footerPanel\""));
    assert!(response.contains("id=\"footerMessages\""));
    assert!(response.contains("id=\"previewPanel\""));
    assert!(response.contains("id=\"traceWorkspace\""));
    assert!(response.contains("id=\"commandPanel\""));
    assert!(response.contains("id=\"commandList\""));
}

#[test]
fn web_mode_serves_the_new_spa_beside_the_existing_viewer() {
    let port = unused_loopback_port();
    let mut child = start_case_web("single-sheet", port);

    wait_until_listening(&mut child, port);
    let legacy = http_get_bytes(port, "/");
    let redirect = http_get_bytes(port, "/app");
    let app = http_get_bytes(port, "/app/");
    stop(&mut child);

    assert_eq!(response_status(&legacy), "HTTP/1.1 200 OK");
    assert!(
        String::from_utf8_lossy(response_body(&legacy)).contains("<title>ESCPost render</title>")
    );
    assert_eq!(
        response_status(&redirect),
        "HTTP/1.1 308 Permanent Redirect"
    );
    assert_eq!(response_header(&redirect, "location"), Some("/app/"));
    assert_eq!(response_status(&app), "HTTP/1.1 200 OK");
    assert_eq!(response_header(&app, "cache-control"), Some("no-cache"));
    assert!(
        String::from_utf8_lossy(response_body(&app)).contains("<title>ESCPost workbench</title>")
    );
}

#[test]
fn web_mode_serves_hashed_spa_assets_with_immutable_caching() {
    let port = unused_loopback_port();
    let mut child = start_case_web("single-sheet", port);

    wait_until_listening(&mut child, port);
    let app = http_get_bytes(port, "/app/");
    let html = String::from_utf8_lossy(response_body(&app));
    let javascript_path = referenced_asset(&html, ".js");
    let css_path = referenced_asset(&html, ".css");
    let javascript = http_get_bytes(port, &javascript_path);
    let css = http_get_bytes(port, &css_path);
    stop(&mut child);

    assert_eq!(response_status(&javascript), "HTTP/1.1 200 OK");
    assert!(matches!(
        response_header(&javascript, "content-type"),
        Some("text/javascript") | Some("application/javascript")
    ));
    assert_eq!(
        response_header(&javascript, "cache-control"),
        Some("public, max-age=31536000, immutable")
    );
    assert_eq!(response_status(&css), "HTTP/1.1 200 OK");
    assert_eq!(response_header(&css, "content-type"), Some("text/css"));
    assert_eq!(
        response_header(&css, "cache-control"),
        Some("public, max-age=31536000, immutable")
    );
    let javascript = String::from_utf8_lossy(response_body(&javascript));
    for label in [
        "Overview",
        "Print jobs",
        "Printers",
        "Profiles",
        "Calibration",
    ] {
        assert!(
            javascript.contains(label),
            "the production bundle should contain the {label:?} workbench label"
        );
    }
}

#[test]
fn web_mode_rejects_missing_spa_assets_and_asset_traversal() {
    let port = unused_loopback_port();
    let mut child = start_case_web("single-sheet", port);

    wait_until_listening(&mut child, port);
    let missing = http_get_bytes(port, "/app/assets/missing.js");
    let traversal = http_get_bytes(port, "/app/assets/../../Cargo.toml");
    stop(&mut child);

    for response in [&missing, &traversal] {
        assert_eq!(response_status(response), "HTTP/1.1 404 Not Found");
        assert!(!String::from_utf8_lossy(response_body(response)).contains("[workspace]"));
    }
}

#[test]
fn read_only_api_lists_printers_and_profiles() {
    let configuration_directory = temporary_directory("read-only-api");
    fs::write(
        configuration_directory.join("printers.toml"),
        r#"
[kitchen]
transport = "network"
host = "127.0.0.1"
port = 9
profile = "REFERENCE"
"#,
    )
    .expect("the printer fixture should be writable");
    let port = unused_loopback_port();
    let mut child =
        start_case_web_with_config_directory("single-sheet", port, &configuration_directory);

    wait_until_listening(&mut child, port);
    let printers = http_get_bytes(port, "/api/printers/list");
    let network_printers = http_get_bytes(port, "/api/printers/list?transport=network");
    let invalid_transport = http_get_bytes(port, "/api/printers/list?transport=invalid");
    let undeclared_printer_query = http_get_bytes(port, "/api/printers/list?config=host.toml");
    let profiles = http_get_bytes(port, "/api/profiles/list");
    let undeclared_profile_query = http_get_bytes(port, "/api/profiles/list?source=virtual");
    stop(&mut child);
    fs::remove_dir_all(&configuration_directory)
        .expect("the configuration fixture should be removable");

    assert_eq!(response_status(&printers), "HTTP/1.1 200 OK");
    assert_eq!(
        response_header(&printers, "cache-control"),
        Some("no-store")
    );
    let printers: serde_json::Value = serde_json::from_slice(response_body(&printers))
        .expect("the printer response should be JSON");
    assert_eq!(printers["printers"].as_array().map(Vec::len), Some(1));
    assert_eq!(printers["printers"][0]["name"], "kitchen");
    assert_eq!(printers["printers"][0]["transport"], "network");
    assert_eq!(printers["printers"][0]["availability"], "unavailable");
    assert_eq!(printers["printers"][0]["profile"], "REFERENCE");
    assert_eq!(printers["printers"][0]["connection"]["type"], "network");
    assert_eq!(printers["printers"][0]["connection"]["host"], "127.0.0.1");
    assert_eq!(printers["printers"][0]["connection"]["port"], 9);
    assert!(printers.get("config_path").is_none());

    assert_eq!(response_status(&network_printers), "HTTP/1.1 200 OK");
    let network_printers: serde_json::Value =
        serde_json::from_slice(response_body(&network_printers))
            .expect("the filtered printer response should be JSON");
    assert_eq!(network_printers, printers);

    assert_eq!(
        response_status(&invalid_transport),
        "HTTP/1.1 400 Bad Request"
    );
    assert_eq!(
        response_header(&invalid_transport, "cache-control"),
        Some("no-store")
    );
    let invalid_transport: serde_json::Value =
        serde_json::from_slice(response_body(&invalid_transport))
            .expect("the invalid query response should be JSON");
    assert_eq!(invalid_transport["error"]["code"], "invalid_query");
    assert!(invalid_transport["error"]["message"].is_string());

    for response in [&undeclared_printer_query, &undeclared_profile_query] {
        assert_eq!(response_status(response), "HTTP/1.1 400 Bad Request");
        assert_eq!(response_header(response, "cache-control"), Some("no-store"));
        assert!(matches!(
            response_header(response, "content-type"),
            Some(value) if value.starts_with("application/json")
        ));
        let response: serde_json::Value = serde_json::from_slice(response_body(response))
            .expect("invalid query responses should be JSON");
        assert_eq!(response["error"]["code"], "invalid_query");
        assert!(response["error"]["message"].is_string());
    }

    assert_eq!(response_status(&profiles), "HTTP/1.1 200 OK");
    assert_eq!(
        response_header(&profiles, "cache-control"),
        Some("no-store")
    );
    let profiles: serde_json::Value = serde_json::from_slice(response_body(&profiles))
        .expect("the profile response should be JSON");
    let profiles = profiles["profiles"]
        .as_array()
        .expect("profiles should be an array");
    assert!(profiles.windows(2).all(|pair| {
        pair[0]["id"]
            .as_str()
            .expect("profile IDs should be strings")
            <= pair[1]["id"]
                .as_str()
                .expect("profile IDs should be strings")
    }));
    let reference = profiles
        .iter()
        .find(|profile| profile["id"] == "REFERENCE")
        .expect("the reference profile should be present");
    assert_eq!(reference["source"], "virtual");
    for field in [
        "vendor",
        "model",
        "paper_width_mm",
        "printable_width_mm",
        "printable_width_dots",
        "dpi_x",
        "dpi_y",
        "full_cut",
        "partial_cut",
        "barcode_function_a",
        "barcode_function_b",
        "qr_code",
    ] {
        assert!(
            reference.get(field).is_some(),
            "REFERENCE should expose {field}"
        );
    }
}

#[test]
fn known_api_routes_reject_non_get_methods_with_json_errors() {
    let port = unused_loopback_port();
    let mut child = start_case_web("single-sheet", port);

    wait_until_listening(&mut child, port);
    let paths = ["/api/status", "/api/printers/list", "/api/profiles/list"];
    let responses: Vec<Vec<u8>> = ["POST", "PUT", "PATCH", "DELETE"]
        .into_iter()
        .flat_map(|method| {
            paths
                .iter()
                .map(move |path| http_request_bytes(port, method, path))
        })
        .collect();
    stop(&mut child);

    for response in responses {
        assert_eq!(
            response_status(&response),
            "HTTP/1.1 405 Method Not Allowed"
        );
        assert_eq!(
            response_header(&response, "cache-control"),
            Some("no-store")
        );
        assert_eq!(response_header(&response, "allow"), Some("GET, HEAD"));
        assert!(matches!(
            response_header(&response, "content-type"),
            Some(value) if value.starts_with("application/json")
        ));
        let response: serde_json::Value = serde_json::from_slice(response_body(&response))
            .expect("method failures should be JSON");
        assert_eq!(response["error"]["code"], "method_not_allowed");
        assert_eq!(
            response["error"]["message"],
            "This API endpoint only accepts GET and HEAD requests."
        );
    }
}

#[test]
fn direct_spa_navigation_uses_index_without_catching_unknown_api_routes() {
    let port = unused_loopback_port();
    let mut child = start_case_web("single-sheet", port);

    wait_until_listening(&mut child, port);
    let index = http_get_bytes(port, "/app/");
    let jobs = http_get_bytes(port, "/app/jobs");
    let printers = http_get_bytes(port, "/app/printers");
    let profiles = http_get_bytes(port, "/app/profiles");
    let calibration = http_get_bytes(port, "/app/calibration");
    let unknown = http_get_bytes(port, "/app/unknown");
    let unknown_api = http_get_bytes(port, "/api/unknown");
    stop(&mut child);

    for response in [&index, &jobs, &printers, &profiles, &calibration, &unknown] {
        assert_eq!(response_status(response), "HTTP/1.1 200 OK");
        assert_eq!(response_header(response, "cache-control"), Some("no-cache"));
        assert_eq!(response_body(response), response_body(&index));
    }

    assert_eq!(response_status(&unknown_api), "HTTP/1.1 404 Not Found");
    assert_eq!(
        response_header(&unknown_api, "cache-control"),
        Some("no-store")
    );
    assert!(matches!(
        response_header(&unknown_api, "content-type"),
        Some(value) if value.starts_with("application/json")
    ));
    let unknown_api: serde_json::Value = serde_json::from_slice(response_body(&unknown_api))
        .expect("unknown API responses should be JSON");
    assert_eq!(unknown_api["error"]["code"], "not_found");
    assert!(unknown_api["error"]["message"].is_string());
}

#[test]
fn unknown_non_get_api_route_uses_the_json_not_found_envelope() {
    let port = unused_loopback_port();
    let mut child = start_case_web("single-sheet", port);

    wait_until_listening(&mut child, port);
    let response = http_post_bytes(port, "/api/unknown");
    stop(&mut child);

    assert_eq!(response_status(&response), "HTTP/1.1 404 Not Found");
    assert_eq!(
        response_header(&response, "cache-control"),
        Some("no-store")
    );
    assert!(matches!(
        response_header(&response, "content-type"),
        Some(value) if value.starts_with("application/json")
    ));
    let response: serde_json::Value = serde_json::from_slice(response_body(&response))
        .expect("unknown API responses should be JSON");
    assert_eq!(response["error"]["code"], "not_found");
    assert!(response["error"]["message"].is_string());
}

#[test]
fn health_endpoint_reports_ok() {
    let port = unused_loopback_port();
    let mut child = start_case_web("single-sheet", port);

    wait_until_listening(&mut child, port);
    let response = http_get_bytes(port, "/health");
    stop(&mut child);

    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
    assert_eq!(response_body(&response), b"ok");
}

#[test]
fn api_status_has_no_virtual_printer_for_render_web_mode() {
    let port = unused_loopback_port();
    let mut child = start_case_web("single-sheet", port);

    wait_until_listening(&mut child, port);
    let response = http_get_bytes(port, "/api/status");
    let status: serde_json::Value = serde_json::from_slice(response_body(&response))
        .expect("the status response should be JSON");
    stop(&mut child);

    assert_eq!(response_status(&response), "HTTP/1.1 200 OK");
    assert_eq!(
        response_header(&response, "cache-control"),
        Some("no-store")
    );
    assert!(matches!(
        response_header(&response, "content-type"),
        Some(value) if value.starts_with("application/json")
    ));
    assert_eq!(status["virtual_printer"], serde_json::Value::Null);
    assert_eq!(status["jobs_processed"], 0);
}

#[test]
fn web_mode_exposes_ordered_sheet_metadata_and_png_bytes() {
    let port = unused_loopback_port();
    let case = "multi-sheet";
    let mut child = start_case_web(case, port);

    wait_until_listening(&mut child, port);
    let metadata_response = http_get_bytes(port, "/api/render");
    let metadata: serde_json::Value = serde_json::from_slice(response_body(&metadata_response))
        .expect("the metadata response should be JSON");
    let sheet_names: Vec<&str> = metadata["sheets"]
        .as_array()
        .expect("sheets should be an array")
        .iter()
        .map(|sheet| sheet["name"].as_str().expect("a sheet should have a name"))
        .collect();
    assert_eq!(
        sheet_names,
        ["sheet-001.png", "sheet-002.png", "sheet-003.png"]
    );
    assert_eq!(metadata["profile"], "REFERENCE");

    let png_response = http_get_bytes(port, "/sheets/2.png");
    stop(&mut child);
    assert_eq!(&response_body(&png_response)[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn web_mode_exposes_experimental_command_traces() {
    let temporary_directory = temporary_directory("command-trace");
    let input_path = temporary_directory.join("receipt.bin");
    let qr_content = b"https://example.test";
    let mut input = vec![0x1b, b'a', 1, b'A', 0x0a];
    input.extend_from_slice(&[
        0x1d,
        b'(',
        b'k',
        (qr_content.len() + 3) as u8,
        0,
        49,
        80,
        48,
    ]);
    input.extend_from_slice(qr_content);
    input.extend_from_slice(&[0x1d, b'(', b'k', 3, 0, 49, 81, 48]);
    fs::write(&input_path, input).expect("the traced input should be writable");
    let port = unused_loopback_port();
    let mut child = start_file_web(&input_path, port, false);

    wait_until_listening(&mut child, port);
    let response = http_get_bytes(port, "/api/render");
    let metadata: serde_json::Value = serde_json::from_slice(response_body(&response))
        .expect("the render response should be JSON");
    stop(&mut child);

    assert!(metadata.get("commands").is_none());
    let commands = metadata["sheets"][0]["commands"]
        .as_array()
        .expect("commands should be an array");
    assert_eq!(commands.len(), 5);
    assert_eq!(commands[0]["byte_start"], 0);
    assert_eq!(commands[0]["byte_end"], 3);
    assert_eq!(commands[0]["name"], "ESC a");
    assert_eq!(commands[0]["detail"], "Set justification: center");
    assert!(commands[0].get("paint_lifecycle").is_none());
    assert_eq!(commands[0]["effects"][0]["type"], "state_change");
    assert_eq!(commands[1]["name"], "Text");
    assert_eq!(commands[1]["detail"], "A");
    assert_eq!(commands[1]["paint_lifecycle"], "committed");
    let bounds = &commands[1]["effects"][0]["bounds"];
    assert_eq!(bounds["x"], 282);
    assert_eq!(bounds["y"], 0);
    assert_eq!(bounds["width"], 12);
    assert_eq!(bounds["height"], 24);
    assert_eq!(commands[2]["name"], "LF");
    assert_eq!(commands[2]["effects"][0]["type"], "motion");
    assert_eq!(commands[3]["name"], "GS (");
    assert_eq!(
        commands[3]["detail"],
        "Parsed command · annotations not yet modeled"
    );
    assert_eq!(commands[3]["effects"].as_array().unwrap().len(), 0);
    assert_eq!(commands[4]["name"], "GS ( k");
    assert_eq!(commands[4]["detail"], "Print QR code · Function 181");
    assert_eq!(commands[4]["paint_lifecycle"], "committed");
    assert_eq!(commands[4]["annotation"]["label"], "https://example.test");
    assert_eq!(commands[4]["annotation"]["content"], "https://example.test");

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn web_mode_lists_buffered_text_without_fabricating_a_sheet_image() {
    let temporary_directory = temporary_directory("buffered-command-trace");
    let input_path = temporary_directory.join("receipt.bin");
    fs::write(&input_path, b"A").expect("the buffered input should be writable");
    let port = unused_loopback_port();
    let mut child = start_file_web(&input_path, port, false);

    wait_until_listening(&mut child, port);
    let response = http_get_bytes(port, "/api/render");
    let metadata: serde_json::Value = serde_json::from_slice(response_body(&response))
        .expect("the render response should be JSON");
    stop(&mut child);

    let sheets = metadata["sheets"]
        .as_array()
        .expect("conceptual sheets should be an array");
    assert_eq!(sheets.len(), 1);
    assert!(sheets[0].get("url").is_none());
    assert!(sheets[0].get("width_dots").is_none());
    assert!(sheets[0].get("height_dots").is_none());
    let commands = sheets[0]["commands"]
        .as_array()
        .expect("commands should be an array");
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0]["name"], "Text");
    assert_eq!(commands[0]["paint_lifecycle"], "buffered");
    assert!(commands[0]["effects"].as_array().unwrap().is_empty());

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn empty_job_web_mode_exposes_an_ordered_empty_sheet_list() {
    let temporary_directory = temporary_directory("empty-job");
    let input_path = temporary_directory.join("empty.bin");
    fs::write(&input_path, []).expect("the empty input should be writable");
    let port = unused_loopback_port();
    let mut child = start_file_web(&input_path, port, false);

    wait_until_listening(&mut child, port);
    let metadata_response = http_get_bytes(port, "/api/render");
    let metadata: serde_json::Value = serde_json::from_slice(response_body(&metadata_response))
        .expect("the metadata response should be JSON");
    stop(&mut child);

    assert_eq!(metadata["profile"], "REFERENCE");
    assert_eq!(
        metadata["sheets"]
            .as_array()
            .expect("sheets should be an array")
            .len(),
        0
    );
    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn web_mode_can_publish_the_same_complete_render_to_a_file() {
    let temporary_directory = temporary_directory("file-and-web");
    let input_path = temporary_directory.join("receipt.bin");
    let output_path = temporary_directory.join("receipt.png");
    fs::write(&input_path, b"Two destinations\n").expect("the input should be writable");
    let port = unused_loopback_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            input_path.to_str().expect("the input path should be UTF-8"),
            "--profile",
            "REFERENCE",
            "--output",
            output_path
                .to_str()
                .expect("the output path should be UTF-8"),
            "--web-listen",
            &format!("127.0.0.1:{port}"),
            "--non-interactive",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the escpost command should start");

    wait_until_listening(&mut child, port);
    let served_png = response_body(&http_get_bytes(port, "/sheets/1.png")).to_vec();
    stop(&mut child);

    assert_eq!(
        fs::read(&output_path).expect("the persisted PNG should exist"),
        served_png
    );
    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn explicit_occupied_web_port_fails_instead_of_falling_back() {
    let occupied =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a port should be reservable");
    let port = occupied
        .local_addr()
        .expect("the listener should have an address")
        .port();
    let case_directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cases/single-sheet");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            case_directory
                .to_str()
                .expect("the case path should be UTF-8"),
            "--web-listen",
            &format!("127.0.0.1:{port}"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should finish");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains(&port.to_string()));
}

#[test]
fn explicit_port_zero_reports_the_operating_system_selected_port() {
    let mut child = start_case_web("single-sheet", 0);
    let mut stderr = BufReader::new(
        child
            .stderr
            .take()
            .expect("the web command stderr should be piped"),
    );
    let viewer_line = loop {
        let mut line = String::new();
        stderr
            .read_line(&mut line)
            .expect("web status should be readable");
        assert!(!line.is_empty(), "web viewer URL should be reported");
        if line.starts_with("Web viewer: ") {
            break line;
        }
    };
    let port = viewer_line
        .trim()
        .strip_prefix("Web viewer: http://127.0.0.1:")
        .and_then(|value| value.strip_suffix('/'))
        .and_then(|value| value.parse::<u16>().ok())
        .expect("the web viewer status should contain the selected port");

    let response = http_get(port, "/");
    stop(&mut child);

    assert_ne!(port, 0);
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
}

#[test]
fn watch_mode_replaces_the_render_after_the_source_changes() {
    let temporary_directory = temporary_directory("watch");
    let input_path = temporary_directory.join("receipt.bin");
    fs::write(&input_path, b"Before\n").expect("the initial input should be writable");
    let port = unused_loopback_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            input_path.to_str().expect("the input path should be UTF-8"),
            "--profile",
            "REFERENCE",
            "--watch",
            "--web-listen",
            &format!("127.0.0.1:{port}"),
            "--non-interactive",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the escpost command should start");

    wait_until_listening(&mut child, port);
    let before = response_body(&http_get_bytes(port, "/sheets/1.png")).to_vec();
    fs::write(&input_path, b"After\n").expect("the changed input should be writable");
    let deadline = Instant::now() + Duration::from_secs(5);
    let after = loop {
        let candidate = response_body(&http_get_bytes(port, "/sheets/1.png")).to_vec();
        if candidate != before {
            break candidate;
        }
        assert!(
            Instant::now() < deadline,
            "the watched rendering did not change"
        );
        thread::sleep(Duration::from_millis(50));
    };
    stop(&mut child);

    assert_eq!(&after[..8], b"\x89PNG\r\n\x1a\n");
    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn watch_error_keeps_the_last_complete_render_available() {
    let temporary_directory = temporary_directory("watch-error");
    let input_path = temporary_directory.join("receipt.bin");
    fs::write(&input_path, b"Complete\n").expect("the initial input should be writable");
    let port = unused_loopback_port();
    let mut child = start_file_web(&input_path, port, true);

    wait_until_listening(&mut child, port);
    let previous_png = response_body(&http_get_bytes(port, "/sheets/1.png")).to_vec();
    fs::write(&input_path, b"\x1b").expect("the invalid input should be writable");
    let deadline = Instant::now() + Duration::from_secs(5);
    let error = loop {
        let response = http_get_bytes(port, "/api/render");
        let metadata: serde_json::Value = serde_json::from_slice(response_body(&response))
            .expect("the metadata should remain JSON");
        if let Some(error) = metadata["error"].as_str() {
            break error.to_owned();
        }
        assert!(
            Instant::now() < deadline,
            "the watched render error did not become visible"
        );
        thread::sleep(Duration::from_millis(50));
    };
    let still_available = response_body(&http_get_bytes(port, "/sheets/1.png")).to_vec();
    stop(&mut child);

    assert!(error.contains("truncated ESC command"));
    assert_eq!(still_available, previous_png);
    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn watch_mode_rejects_stdin_as_a_mutable_source() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            "-",
            "--format",
            "binary",
            "--profile",
            "REFERENCE",
            "--watch",
            "--non-interactive",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("the escpost command should finish");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("watch mode requires a filesystem source")
    );
}

#[test]
fn web_mode_does_not_serve_missing_sheets_or_filesystem_paths() {
    let port = unused_loopback_port();
    let mut child = start_case_web("single-sheet", port);

    wait_until_listening(&mut child, port);
    let missing = http_get(port, "/sheets/999.png");
    let traversal = http_get(port, "/sheets/../../Cargo.toml");
    stop(&mut child);

    assert!(missing.starts_with("HTTP/1.1 404 Not Found\r\n"));
    assert!(traversal.starts_with("HTTP/1.1 404 Not Found\r\n"));
    assert!(!traversal.contains("[workspace]"));
}

#[test]
fn browser_mode_starts_the_same_web_viewer() {
    let port = unused_loopback_port();
    let case_directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cases/single-sheet");
    let mut child = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            case_directory
                .to_str()
                .expect("the case path should be UTF-8"),
            "--browser",
            "--web-listen",
            &format!("127.0.0.1:{port}"),
            "--non-interactive",
        ])
        // webbrowser honors BROWSER on Linux. A harmless executable verifies
        // the launch path without opening a GUI during automated tests.
        .env("BROWSER", "/bin/true")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the escpost command should start");

    wait_until_listening(&mut child, port);
    let response = http_get(port, "/");
    stop(&mut child);

    assert!(response.contains("<title>ESCPost render</title>"));
}

#[test]
fn non_loopback_listener_prints_a_receipt_exposure_warning() {
    let port = unused_loopback_port();
    let case_directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cases/single-sheet");
    let mut child = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            case_directory
                .to_str()
                .expect("the case path should be UTF-8"),
            "--web-listen",
            &format!("0.0.0.0:{port}"),
            "--non-interactive",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the escpost command should start");

    wait_until_listening(&mut child, port);
    stop(&mut child);
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr should be piped")
        .read_to_string(&mut stderr)
        .expect("stderr should be readable");

    assert!(stderr.contains("warning: receipt data is exposed beyond loopback"));
}

fn start_case_web(case: &str, port: u16) -> Child {
    let case_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cases")
        .join(case);
    Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            case_directory
                .to_str()
                .expect("the case path should be UTF-8"),
            "--web",
            "--web-listen",
            &format!("127.0.0.1:{port}"),
            "--non-interactive",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the escpost command should start")
}

fn start_case_web_with_config_directory(case: &str, port: u16, config_directory: &Path) -> Child {
    let case_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cases")
        .join(case);
    Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            case_directory
                .to_str()
                .expect("the case path should be UTF-8"),
            "--web",
            "--web-listen",
            &format!("127.0.0.1:{port}"),
            "--non-interactive",
        ])
        .env("ESCPOST_CONFIG_DIR", config_directory)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the escpost command should start")
}

fn start_file_web(input_path: &Path, port: u16, watch: bool) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_escpost"));
    command.args([
        "render",
        input_path.to_str().expect("the input path should be UTF-8"),
        "--profile",
        "REFERENCE",
        "--web-listen",
        &format!("127.0.0.1:{port}"),
        "--non-interactive",
    ]);
    if watch {
        command.arg("--watch");
    }
    command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the escpost command should start")
}

fn unused_loopback_port() -> u16 {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .expect("an ephemeral loopback port should be available")
        .local_addr()
        .expect("the listener should have a local address")
        .port()
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
            let mut stderr = String::new();
            child
                .stderr
                .take()
                .expect("stderr should be piped")
                .read_to_string(&mut stderr)
                .expect("stderr should be readable");
            panic!("web command exited early with {status}:\n{stderr}");
        }
        assert!(
            Instant::now() < deadline,
            "web command did not listen on port {port}"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

fn http_get(port: u16, path: &str) -> String {
    String::from_utf8(http_get_bytes(port, path)).expect("the HTTP response should be UTF-8")
}

fn http_get_bytes(port: u16, path: &str) -> Vec<u8> {
    http_request_bytes(port, "GET", path)
}

fn http_post_bytes(port: u16, path: &str) -> Vec<u8> {
    http_request_bytes(port, "POST", path)
}

fn http_request_bytes(port: u16, method: &str, path: &str) -> Vec<u8> {
    let mut stream =
        TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("the web server should accept HTTP");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .expect("the HTTP request should be writable");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .expect("the HTTP response should be readable");
    response
}

fn response_body(response: &[u8]) -> &[u8] {
    let boundary = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the HTTP response should contain a header boundary");
    &response[boundary + 4..]
}

fn response_status(response: &[u8]) -> &str {
    let response = std::str::from_utf8(response).expect("HTTP response should be UTF-8");
    response
        .lines()
        .next()
        .expect("HTTP response should have a status line")
}

fn response_header<'a>(response: &'a [u8], requested: &str) -> Option<&'a str> {
    let response = std::str::from_utf8(response).ok()?;
    let head = response.split_once("\r\n\r\n")?.0;
    head.lines().skip(1).find_map(|line| {
        let (name, value) = line.split_once(": ")?;
        name.eq_ignore_ascii_case(requested).then_some(value)
    })
}

fn referenced_asset(html: &str, extension: &str) -> String {
    html.split('"')
        .find(|part| part.starts_with("/app/assets/") && part.ends_with(extension))
        .unwrap_or_else(|| panic!("SPA document should reference a {extension} asset"))
        .to_owned()
}

fn stop(child: &mut Child) {
    child.kill().expect("the web command should be stoppable");
    child.wait().expect("the web command should be reapable");
}

fn temporary_directory(case: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock should be after the Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "escpost-web-{case}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&path).expect("the test directory should be creatable");
    path
}
