use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn render_help_contract_is_unchanged() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["render", "--help"])
        .output()
        .expect("the escpost command should finish");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("render help should be UTF-8"),
        "\
Render a known ESC/POS byte stream

Usage: escpost render [OPTIONS] <SOURCE>

Arguments:
  <SOURCE>  Raw ESC/POS file, hexadecimal file, case directory, or - for stdin

Options:
      --format <FORMAT>          Input representation [default: auto] [possible values: auto, binary, hex]
      --non-interactive          Never prompt for missing values
      --profile <PROFILE>        Printer profile used to interpret the input
  -o, --output <OUTPUT>          Write one PNG to this path, or use - for stdout
      --output-dir <OUTPUT_DIR>  Write every rendered sheet and a manifest to this directory
      --sheet <SHEET>            Select one one-based sheet for single-PNG output
      --web                      Start the local web viewer and keep running
      --browser                  Start the web viewer and open it in the default browser
      --web-listen <WEB_LISTEN>  Exact address for the web viewer
      --watch                    Rerender a filesystem source whenever it changes
      --scale <N>                Output pixel density: 1 to 3 subpixels per dot. 1 is dot resolution [default: 1]
      --antialias [<ANTIALIAS>]  Anti-alias glyph edges into a grayscale preview (cosmetic; never what a printer emits). Pass --antialias for a nicer on-screen render [default: false] [possible values: true, false]
  -h, --help                     Print help
"
    );
}

#[test]
fn render_rejects_an_unsupported_scale_before_reading_the_source() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            "/path/that/does/not/exist",
            "--profile",
            "REFERENCE",
            "--output",
            "ignored.png",
            "--scale",
            "4",
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should finish");

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: render scale must be between 1 and 3, got 4\n"
    );
}

#[test]
fn raw_file_renders_one_png_with_an_explicit_profile() {
    let temporary_directory = temporary_directory("raw-file");
    let input_path = temporary_directory.join("receipt.bin");
    let output_path = temporary_directory.join("receipt.png");
    fs::write(&input_path, b"Hello\n").expect("the input fixture should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            input_path.to_str().expect("the input path should be UTF-8"),
            "--profile",
            "REFERENCE",
            "--output",
            output_path
                .to_str()
                .expect("the output path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should start");

    assert!(
        output.status.success(),
        "render failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        &fs::read(&output_path).expect("the output PNG should exist")[..8],
        b"\x89PNG\r\n\x1a\n"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Profile: REFERENCE"),
        "file output should report the profile on stderr"
    );

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn hex_extension_decodes_readable_fixture_bytes() {
    let temporary_directory = temporary_directory("hex-file");
    let binary_input = temporary_directory.join("receipt.bin");
    let hex_input = temporary_directory.join("receipt.hex");
    let binary_output = temporary_directory.join("binary.png");
    let hex_output = temporary_directory.join("hex.png");
    fs::write(&binary_input, b"Hi\n").expect("the binary fixture should be writable");
    fs::write(&hex_input, "48 69 0a\n").expect("the hex fixture should be writable");

    render_file(&binary_input, &binary_output);
    render_file(&hex_input, &hex_output);

    assert_eq!(
        fs::read(hex_output).expect("the hex rendering should exist"),
        fs::read(binary_output).expect("the binary rendering should exist")
    );

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn explicit_hex_format_does_not_depend_on_the_filename_extension() {
    let temporary_directory = temporary_directory("explicit-hex");
    let binary_input = temporary_directory.join("receipt.bin");
    let readable_input = temporary_directory.join("receipt.data");
    let binary_output = temporary_directory.join("binary.png");
    let readable_output = temporary_directory.join("readable.png");
    fs::write(&binary_input, b"Hi\n").expect("the binary fixture should be writable");
    fs::write(&readable_input, "48 69 0a\n").expect("the hex fixture should be writable");

    render_file(&binary_input, &binary_output);
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            readable_input
                .to_str()
                .expect("the input path should be UTF-8"),
            "--format",
            "hex",
            "--profile",
            "REFERENCE",
            "--output",
            readable_output
                .to_str()
                .expect("the output path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should start");

    assert!(
        output.status.success(),
        "render failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(readable_output).expect("the explicit hex rendering should exist"),
        fs::read(binary_output).expect("the binary rendering should exist")
    );

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn option_terminator_allows_a_source_name_that_starts_with_a_hyphen() {
    let temporary_directory = temporary_directory("option-terminator");
    let input_path = temporary_directory.join("-receipt.bin");
    let output_path = temporary_directory.join("receipt.png");
    fs::write(&input_path, b"Options\n").expect("the input fixture should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .current_dir(&temporary_directory)
        .args([
            "render",
            "--profile",
            "REFERENCE",
            "--output",
            output_path
                .to_str()
                .expect("the output path should be UTF-8"),
            "--non-interactive",
            "--",
            "-receipt.bin",
        ])
        .output()
        .expect("the escpost command should start");

    assert!(
        output.status.success(),
        "render failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        &fs::read(output_path).expect("the output PNG should exist")[..8],
        b"\x89PNG\r\n\x1a\n"
    );

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn case_directory_supplies_input_and_profile_metadata() {
    let temporary_directory = temporary_directory("case-directory");
    let output_path = temporary_directory.join("case.png");
    let case_directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cases/single-sheet");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            case_directory
                .to_str()
                .expect("the case path should be UTF-8"),
            "--output",
            output_path
                .to_str()
                .expect("the output path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should start");

    assert!(
        output.status.success(),
        "render failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        &fs::read(output_path).expect("the case PNG should exist")[..8],
        b"\x89PNG\r\n\x1a\n"
    );

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn explicit_profile_takes_precedence_over_case_metadata() {
    let temporary_directory = temporary_directory("profile-precedence");
    let output_path = temporary_directory.join("case.png");
    let case_directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cases/single-sheet");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            case_directory
                .to_str()
                .expect("the case path should be UTF-8"),
            "--profile",
            "NT-5890K",
            "--output",
            output_path
                .to_str()
                .expect("the output path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should start");

    assert!(
        output.status.success(),
        "render failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Profile: NT-5890K"),
        "the explicit profile should win over case metadata"
    );

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn arbitrary_directory_is_not_treated_as_an_escpos_source() {
    let temporary_directory = temporary_directory("arbitrary-directory");
    let output_path = temporary_directory.join("receipt.png");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            temporary_directory
                .to_str()
                .expect("the input path should be UTF-8"),
            "--profile",
            "REFERENCE",
            "--output",
            output_path
                .to_str()
                .expect("the output path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should finish");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("directory is not a recognized ESCPost case")
    );
    assert!(!output_path.exists());

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn stdin_to_stdout_contains_only_one_png() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            "-",
            "--format",
            "binary",
            "--profile",
            "REFERENCE",
            "--output",
            "-",
            "--non-interactive",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the escpost command should start");
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(b"Pipe\n")
        .expect("the fixture should be writable to stdin");

    let output = child
        .wait_with_output()
        .expect("the escpost command should finish");

    assert!(
        output.status.success(),
        "render failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(&output.stdout[..8], b"\x89PNG\r\n\x1a\n");
    assert!(
        output.stderr.is_empty(),
        "successful binary output should not emit status text: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn empty_job_cannot_be_written_to_stdout_as_a_png() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            "-",
            "--format",
            "binary",
            "--profile",
            "REFERENCE",
            "--output",
            "-",
            "--non-interactive",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the escpost command should start");
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .expect("the escpost command should finish");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("single-PNG output requires exactly one sheet")
    );
}

#[test]
fn multi_sheet_job_cannot_be_concatenated_on_stdout() {
    let case_directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cases/multi-sheet");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            case_directory
                .to_str()
                .expect("the case path should be UTF-8"),
            "--output",
            "-",
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should finish");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("single-PNG output requires exactly one sheet")
    );
}

#[test]
fn stdout_png_cannot_be_combined_with_web_mode() {
    let input_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cases/single-sheet");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            input_path.to_str().expect("the input path should be UTF-8"),
            "--output",
            "-",
            "--web",
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should finish");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("PNG stdout cannot be combined with a long-running web viewer")
    );
}

#[test]
fn output_directory_writes_every_sheet_and_ordered_manifest() {
    let temporary_directory = temporary_directory("all-sheets");
    let output_directory = temporary_directory.join("rendered");
    let case_directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cases/multi-sheet");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            case_directory
                .to_str()
                .expect("the case path should be UTF-8"),
            "--output-dir",
            output_directory
                .to_str()
                .expect("the output path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should start");

    assert!(
        output.status.success(),
        "render failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(output_directory.join("manifest.json"))
            .expect("the manifest should exist"),
        "{\n  \"sheets\": [\n    \"sheet-001.png\",\n    \"sheet-002.png\",\n    \"sheet-003.png\"\n  ]\n}\n"
    );
    for sheet in ["sheet-001.png", "sheet-002.png", "sheet-003.png"] {
        assert_eq!(
            &fs::read(output_directory.join(sheet)).expect("every listed PNG should exist")[..8],
            b"\x89PNG\r\n\x1a\n"
        );
    }

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn sheet_selection_writes_one_sheet_from_a_multi_sheet_job() {
    let temporary_directory = temporary_directory("selected-sheet");
    let output_path = temporary_directory.join("second.png");
    let case_directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cases/multi-sheet");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            case_directory
                .to_str()
                .expect("the case path should be UTF-8"),
            "--sheet",
            "2",
            "--output",
            output_path
                .to_str()
                .expect("the output path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should start");

    assert!(
        output.status.success(),
        "render failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        &fs::read(output_path).expect("the selected output should exist")[..8],
        b"\x89PNG\r\n\x1a\n"
    );

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn global_non_interactive_flag_is_accepted_before_the_subcommand() {
    let temporary_directory = temporary_directory("global-option");
    let input_path = temporary_directory.join("receipt.bin");
    let output_path = temporary_directory.join("receipt.png");
    fs::write(&input_path, b"Global\n").expect("the input fixture should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "--non-interactive",
            "render",
            input_path.to_str().expect("the input path should be UTF-8"),
            "--profile",
            "REFERENCE",
            "--output",
            output_path
                .to_str()
                .expect("the output path should be UTF-8"),
        ])
        .output()
        .expect("the escpost command should start");

    assert!(
        output.status.success(),
        "render failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn explicit_output_is_replaced_only_after_rendering_succeeds() {
    let temporary_directory = temporary_directory("overwrite");
    let input_path = temporary_directory.join("receipt.bin");
    let output_path = temporary_directory.join("receipt.png");
    fs::write(&input_path, b"\x1b").expect("the invalid fixture should be writable");
    fs::write(&output_path, b"previous").expect("the previous output should be writable");

    let failed = render_process(&input_path, &output_path);

    assert!(!failed.status.success(), "truncated ESC should fail");
    assert_eq!(
        fs::read(&output_path).expect("the previous output should remain"),
        b"previous"
    );

    fs::write(&input_path, b"Replacement\n").expect("the valid fixture should be writable");
    render_file(&input_path, &output_path);
    assert_eq!(
        &fs::read(&output_path).expect("the output should be replaced")[..8],
        b"\x89PNG\r\n\x1a\n"
    );

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn empty_job_writes_an_empty_all_sheet_manifest() {
    let temporary_directory = temporary_directory("empty-job");
    let input_path = temporary_directory.join("empty.bin");
    let output_directory = temporary_directory.join("rendered");
    fs::write(&input_path, []).expect("the empty fixture should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            input_path.to_str().expect("the input path should be UTF-8"),
            "--profile",
            "REFERENCE",
            "--output-dir",
            output_directory
                .to_str()
                .expect("the output path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should start");

    assert!(
        output.status.success(),
        "render failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(output_directory.join("manifest.json"))
            .expect("the manifest should exist"),
        "{\n  \"sheets\": []\n}\n"
    );

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn empty_job_cannot_be_written_as_a_single_png() {
    let temporary_directory = temporary_directory("empty-single-output");
    let input_path = temporary_directory.join("empty.bin");
    let output_path = temporary_directory.join("receipt.png");
    fs::write(&input_path, []).expect("the empty fixture should be writable");

    let output = render_process(&input_path, &output_path);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("single-PNG output requires exactly one sheet")
    );
    assert!(!output_path.exists());

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn single_png_destination_rejects_an_unselected_multi_sheet_job() {
    let temporary_directory = temporary_directory("multi-sheet-error");
    let output_path = temporary_directory.join("receipt.png");
    let case_directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cases/multi-sheet");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            case_directory
                .to_str()
                .expect("the case path should be UTF-8"),
            "--output",
            output_path
                .to_str()
                .expect("the output path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should start");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("single-PNG output requires exactly one sheet")
    );
    assert!(!output_path.exists());

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn non_interactive_mode_reports_a_missing_profile_without_prompting() {
    let temporary_directory = temporary_directory("missing-profile");
    let input_path = temporary_directory.join("receipt.bin");
    let output_path = temporary_directory.join("receipt.png");
    fs::write(&input_path, b"Profile\n").expect("the fixture should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            input_path.to_str().expect("the input path should be UTF-8"),
            "--output",
            output_path
                .to_str()
                .expect("the output path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should finish without prompting");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("printer profile is required; pass --profile REFERENCE")
    );

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn piped_stdin_policy_reports_a_missing_profile_without_an_explicit_flag() {
    let temporary_directory = temporary_directory("effective-non-interactive");
    let output_path = temporary_directory.join("receipt.png");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            "-",
            "--output",
            output_path
                .to_str()
                .expect("the output path should be UTF-8"),
        ])
        .stdin(Stdio::null())
        .output()
        .expect("the escpost command should finish without prompting");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("printer profile is required; pass --profile REFERENCE")
    );

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

#[test]
fn sheet_selection_requires_a_single_png_destination() {
    let case_directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cases/multi-sheet");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            case_directory
                .to_str()
                .expect("the case path should be UTF-8"),
            "--sheet",
            "2",
            "--web",
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should finish");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--output"));
}

#[test]
fn output_directory_overwrites_current_files_but_preserves_stale_sheets() {
    let temporary_directory = temporary_directory("directory-overwrite");
    let output_directory = temporary_directory.join("rendered");
    let multi_sheet_case =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cases/multi-sheet");
    render_to_directory(&multi_sheet_case, &output_directory);
    let stale_sheet = fs::read(output_directory.join("sheet-002.png"))
        .expect("the second sheet should initially exist");

    let input_path = temporary_directory.join("replacement.bin");
    fs::write(&input_path, b"Replacement\n").expect("the replacement input should be writable");
    render_to_directory(&input_path, &output_directory);

    assert_eq!(
        fs::read_to_string(output_directory.join("manifest.json"))
            .expect("the replacement manifest should exist"),
        "{\n  \"sheets\": [\n    \"sheet-001.png\"\n  ]\n}\n"
    );
    assert_eq!(
        fs::read(output_directory.join("sheet-002.png"))
            .expect("the stale sheet should be preserved"),
        stale_sheet
    );

    fs::remove_dir_all(temporary_directory).expect("the test directory should be removable");
}

fn render_file(input_path: &Path, output_path: &Path) {
    let output = render_process(input_path, output_path);

    assert!(
        output.status.success(),
        "render failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn render_process(input_path: &Path, output_path: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            input_path.to_str().expect("the input path should be UTF-8"),
            "--profile",
            "REFERENCE",
            "--output",
            output_path
                .to_str()
                .expect("the output path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should start")
}

fn render_to_directory(input_path: &Path, output_directory: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "render",
            input_path.to_str().expect("the input path should be UTF-8"),
            "--profile",
            "REFERENCE",
            "--output-dir",
            output_directory
                .to_str()
                .expect("the output path should be UTF-8"),
            "--non-interactive",
        ])
        .output()
        .expect("the escpost command should start");
    assert!(
        output.status.success(),
        "render failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn temporary_directory(case: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock should be after the Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("escpost-{case}-{}-{unique}", std::process::id()));
    fs::create_dir(&path).expect("the test directory should be creatable");
    path
}
