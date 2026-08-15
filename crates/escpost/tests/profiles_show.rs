use std::process::Command;

#[test]
fn profiles_show_prints_vendor_and_calibration_source() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["profiles", "show", "TM-T88III"])
        .output()
        .expect("the escpost command should finish");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "command failed:\n{stdout}");
    assert!(stdout.contains("Epson"), "missing vendor:\n{stdout}");
    assert!(
        stdout.contains("synthesized"),
        "missing calibration source:\n{stdout}"
    );
    for label in [
        "sha256:",
        "paper width:",
        "printable width:",
        "resolution:",
        "font a:",
        "font b:",
        "code pages:",
        "graphics:",
        "cut:",
        "qr code:",
        "drawer pulse:",
        "barcodes (a):",
        "barcodes (b):",
    ] {
        assert!(stdout.contains(label), "missing {label:?}:\n{stdout}");
    }
}

#[test]
fn profiles_show_json_parses_as_a_single_profile_object() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["profiles", "show", "TM-T88III", "--json"])
        .output()
        .expect("the escpost command should finish");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "command failed:\n{stdout}");
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");
    assert!(
        value.is_object(),
        "--json output should be a single object: {value}"
    );
    assert!(
        value.get("canonical_profile_sha256").is_some(),
        "missing canonical_profile_sha256: {value}"
    );
    assert_eq!(
        value["source"], "synthesized",
        "JSON keeps the catalog source vocabulary: {value}"
    );
    for field in [
        "id",
        "vendor",
        "model",
        "source",
        "paper_width_mm",
        "printable_width_mm",
        "printable_width_dots",
        "dpi_x",
        "dpi_y",
        "fonts",
        "features",
        "code_page_count",
        "canonical_profile_sha256",
    ] {
        assert!(value.get(field).is_some(), "missing {field:?}: {value}");
    }
    assert_eq!(
        value
            .as_object()
            .expect("profile should be an object")
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
    assert_eq!(
        value["fonts"]
            .as_object()
            .expect("fonts should be an object")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(
        value["fonts"]["a"]
            .as_object()
            .expect("font a should be an object")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["baseline_dots", "cell_height_dots", "cell_width_dots"]
    );
    assert_eq!(
        value["features"]
            .as_object()
            .expect("features should be an object")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "barcodes",
            "graphics",
            "paper_full_cut",
            "paper_part_cut",
            "pulse_standard",
            "qr_code",
        ]
    );
    assert!(
        value["features"]["barcodes"]["function_b"]
            .as_array()
            .is_some_and(|systems| systems.iter().any(|system| system == "code_128")),
        "function-b barcode systems should retain their JSON names: {value}"
    );
    assert_eq!(
        value["features"]["barcodes"]
            .as_object()
            .expect("barcodes should be an object")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["function_a", "function_b"]
    );
}

#[test]
fn profiles_show_unknown_id_exits_non_zero() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["profiles", "show", "definitely-not-a-profile"])
        .output()
        .expect("the escpost command should finish");

    assert!(
        !output.status.success(),
        "unknown profile id should fail:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
