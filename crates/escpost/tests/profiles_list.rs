use std::process::Command;

#[test]
fn profiles_list_shows_known_ids_and_the_calibration_legend() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["profiles", "list"])
        .output()
        .expect("the escpost command should finish");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "command failed:\n{stdout}");
    assert!(stdout.contains("TM-T88III"), "missing TM-T88III:\n{stdout}");
    assert!(
        stdout.contains("CAL: "),
        "missing calibration legend:\n{stdout}"
    );
    // The table shows tenth precision (NT-5890K's paper width is 57.5 mm), not
    // a rounded whole mm, matching the lossless fixed-point storage.
    assert!(
        stdout.contains("57.5"),
        "expected tenth-precision paper width (57.5) in the table:\n{stdout}"
    );
    assert_eq!(
        stdout
            .lines()
            .next()
            .expect("table should include a header")
            .split_whitespace()
            .collect::<Vec<_>>(),
        [
            "PROFILE", "VENDOR", "MODEL", "CAL", "PAPER", "PRINT", "DOTS", "DPI", "CUT", "BC",
            "QR",
        ],
        "table schema changed:\n{stdout}"
    );
}

#[test]
fn profiles_list_search_narrows_to_matching_ids() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["profiles", "list", "--search", "t88"])
        .output()
        .expect("the escpost command should finish");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "command failed:\n{stdout}");
    assert!(stdout.contains("TM-T88III"), "missing TM-T88III:\n{stdout}");
    assert!(
        !stdout.contains("NT-5890K"),
        "search should exclude non-matching ids:\n{stdout}"
    );
}

#[test]
fn profiles_list_source_filter_selects_only_virtual_profiles() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["profiles", "list", "--source", "virtual"])
        .output()
        .expect("the escpost command should finish");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "command failed:\n{stdout}");
    assert!(stdout.contains("REFERENCE"), "missing REFERENCE:\n{stdout}");
    assert!(
        !stdout.contains("TM-T88III"),
        "virtual filter should exclude synthesized profiles:\n{stdout}"
    );
}

#[test]
fn profiles_list_json_parses_as_an_array_of_profile_objects() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["profiles", "list", "--json"])
        .output()
        .expect("the escpost command should finish");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "command failed:\n{stdout}");
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");
    let array = value.as_array().expect("--json output should be an array");
    assert!(!array.is_empty(), "the array should not be empty");
    assert!(
        array[0].get("id").is_some(),
        "each element should have an id field: {}",
        array[0]
    );
    assert_eq!(
        array[0]
            .as_object()
            .expect("each profile should be an object")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "canonical_profile_sha256",
            "code_page_count",
            "dpi_x",
            "dpi_y",
            "features",
            "fonts",
            "id",
            "model",
            "paper_width_mm",
            "printable_width_dots",
            "printable_width_mm",
            "source",
            "vendor",
        ]
    );
}

#[test]
fn profiles_list_json_with_no_matches_prints_an_empty_json_array() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "profiles",
            "list",
            "--json",
            "--vendor",
            "definitely-not-a-vendor",
        ])
        .output()
        .expect("the escpost command should finish");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "an empty match set is not an error:\n{stdout}"
    );
    assert_eq!(
        stdout.trim(),
        "[]",
        "--json must always emit a JSON array, even when empty:\n{stdout}"
    );
    let value: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("stdout should be valid JSON");
    assert_eq!(
        value.as_array().expect("--json output should be an array"),
        &Vec::<serde_json::Value>::new(),
        "the array should be empty"
    );
}

#[test]
fn profiles_list_json_uses_exact_adapter_barcode_labels() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["profiles", "list", "--search", "TM-T88III", "--json"])
        .output()
        .expect("the escpost command should finish");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "command failed:\n{stdout}");
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");
    assert_eq!(
        value[0]["features"]["barcodes"],
        serde_json::json!({
            "function_a": [
                "upc_a", "upc_e", "ean_13", "ean_8", "code_39", "itf", "codabar"
            ],
            "function_b": [
                "upc_a", "upc_e", "ean_13", "ean_8", "code_39", "itf", "codabar",
                "code_93", "code_128"
            ]
        })
    );
}

#[test]
fn profiles_list_with_no_matches_exits_cleanly_with_a_stderr_note() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["profiles", "list", "--vendor", "doesnotexist"])
        .output()
        .expect("the escpost command should finish");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "an empty match set is not an error:\n{stderr}"
    );
    assert!(
        stderr.contains("no profiles match"),
        "stderr should explain the empty result:\n{stderr}"
    );
}
