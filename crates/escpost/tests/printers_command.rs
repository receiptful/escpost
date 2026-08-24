use std::process::Command;

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::io::{BufRead, BufReader, Read};
#[cfg(unix)]
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::process::{Child, ExitStatus, Output, Stdio};
#[cfg(unix)]
use std::sync::Arc;
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[test]
fn printers_list_is_a_rust_cli_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["printers", "list", "--help"])
        .output()
        .expect("the escpost command should finish");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "command failed:\n{stdout}");
    assert!(stdout.contains("List currently usable printers"));
    assert!(stdout.contains("Usage: escpost printers list"));
    assert!(stdout.contains("--transport <TRANSPORT>"));
}

#[test]
fn printers_add_documents_its_registration_options() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["printers", "add", "--help"])
        .output()
        .expect("the escpost command should finish");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "command failed:\n{stdout}");
    assert!(stdout.contains("Register a printer in the local configuration"));
    assert!(stdout.contains("Usage: escpost printers add [OPTIONS] [NAME]"));
    assert!(stdout.contains("--transport <TRANSPORT>"));
    assert!(stdout.contains("usb"));
    assert!(stdout.contains("network"));
    assert!(stdout.contains("--host <HOST>"));
    assert!(stdout.contains("--port <PORT>"));
    assert!(stdout.contains("--vendor-id <VENDOR_ID>"));
    assert!(stdout.contains("--product-id <PRODUCT_ID>"));
    assert!(stdout.contains("--serial <SERIAL>"));
    assert!(stdout.contains("--profile <PROFILE>"));
    assert!(
        stdout.contains(
            "      --subnet <CIDR>            Scan this network (CIDR notation, for example 10.42.0.0/24) instead of the directly connected networks. May be repeated\n"
        ),
        "the add help must preserve the exact subnet description:\n{stdout}"
    );
}

#[cfg(unix)]
#[test]
fn printers_add_requires_a_terminal_for_usb_registration() {
    let directory = temporary_directory("non-interactive-usb");
    let config = directory.join("printers.toml");

    let output = run_non_interactive_add(&config, &["counter", "--transport", "usb"]);

    assert_failed_without_configuration(
        &output,
        &config,
        "USB printer registration requires an interactive terminal",
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_rejects_a_partial_usb_selector() {
    let directory = temporary_directory("partial-usb-selector");
    let config = directory.join("printers.toml");

    let output = run_non_interactive_add(
        &config,
        &["counter", "--transport", "usb", "--vendor-id", "0x0416"],
    );

    assert_failed_without_configuration(
        &output,
        &config,
        "--vendor-id and --product-id must be given together",
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_rejects_usb_selectors_for_a_network_printer() {
    let directory = temporary_directory("network-usb-selector");
    let config = directory.join("printers.toml");

    let output = run_non_interactive_add(
        &config,
        &[
            "kitchen",
            "--transport",
            "network",
            "--host",
            "10.42.0.71",
            "--vendor-id",
            "0x0416",
            "--product-id",
            "0x5011",
        ],
    );

    assert_failed_without_configuration(&output, &config, "only valid for USB printers");
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_registers_a_network_printer_in_an_explicit_configuration() {
    let directory = temporary_directory("add-network-printer");
    let config = directory.join("printers.toml");

    let output = run_non_interactive_add(
        &config,
        &[
            "kitchen",
            "--transport",
            "network",
            "--host",
            "10.42.0.71",
            "--port",
            "9100",
            "--profile",
            "REFERENCE",
        ],
    );

    assert_command_succeeded(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Printer: kitchen"));
    assert!(stderr.contains("Transport: network"));
    assert!(
        stderr.contains(&format!("Updated configuration at {}", config.display())),
        "the command should report where it saved the printer:\n{stderr}"
    );
    let document = fs::read_to_string(&config).expect("the printer config should be readable");
    let table = toml::from_str::<toml::Table>(&document)
        .expect("the printer config should contain valid TOML");
    let printer = table["kitchen"]
        .as_table()
        .expect("the named printer should be a table");
    assert_eq!(printer["transport"].as_str(), Some("network"));
    assert_eq!(printer["host"].as_str(), Some("10.42.0.71"));
    assert_eq!(printer["port"].as_integer(), Some(9100));
    assert_eq!(printer["profile"].as_str(), Some("REFERENCE"));
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_reports_the_actual_process_local_configuration_path() {
    let directory = temporary_directory("process-local-config");
    let config_directory = directory.join("container");
    fs::create_dir(&config_directory).expect("the config directory should be creatable");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "--non-interactive",
            "printers",
            "add",
            "kitchen",
            "--transport",
            "network",
            "--host",
            "10.42.0.71",
        ])
        .env("ESCPOST_CONFIG_DIR", &config_directory)
        .output()
        .expect("the escpost command should finish");

    assert_command_succeeded(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let displayed = config_directory.join("printers.toml");
    assert!(
        stderr.contains(&format!("Updated configuration at {}", displayed.display())),
        "the reported path should name the path used by the process:\n{stderr}"
    );
    // The file is still written to the actual resolved configuration directory.
    assert!(
        config_directory.join("printers.toml").is_file(),
        "the printer should be saved in the resolved configuration directory"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_list_reports_the_actual_process_local_configuration_path() {
    let directory = temporary_directory("list-process-local-path");
    let config_directory = directory.join("container");
    fs::create_dir(&config_directory).expect("the config directory should be creatable");

    // The network filter keeps the listing from enumerating USB devices, which
    // the test container is not equipped to reach.
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["printers", "list", "--transport", "network"])
        .env("ESCPOST_CONFIG_DIR", &config_directory)
        .output()
        .expect("the escpost command should finish");

    assert_command_succeeded(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let displayed = config_directory.join("printers.toml");
    assert!(
        stderr.contains(&format!(
            "Reading configuration from {}",
            displayed.display()
        )),
        "listing should name the path used by the process:\n{stderr}"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn a_configuration_error_names_the_actual_process_local_path() {
    let directory = temporary_directory("error-process-local-path");
    let config_directory = directory.join("container");
    fs::create_dir(&config_directory).expect("the config directory should be creatable");
    fs::write(config_directory.join("printers.toml"), "this is not TOML")
        .expect("the invalid config should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["printers", "list", "--transport", "network"])
        .env("ESCPOST_CONFIG_DIR", &config_directory)
        .output()
        .expect("the escpost command should finish");

    assert!(!output.status.success(), "invalid configuration must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let displayed = config_directory.join("printers.toml");
    assert!(
        stderr.contains(displayed.to_str().expect("the path should be UTF-8")),
        "the error should name the path used by the process:\n{stderr}"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_preserves_existing_configuration_text() {
    let directory = temporary_directory("preserve-config");
    let config = directory.join("printers.toml");
    let existing = concat!(
        "# Keep this developer note.\n",
        "[netum-usb]\n",
        "transport = \"usb\"\n",
        "profile = \"NT-5890K\"\n",
        "vendor_id = \"0x0416\"\n",
        "product_id = \"0x5011\"\n",
        "interface_number = 0\n",
        "out_endpoint = \"0x01\"\n",
    );
    fs::write(&config, existing).expect("the existing config should be writable");

    let output = run_non_interactive_add(
        &config,
        &["kitchen", "--transport", "network", "--host", "10.42.0.71"],
    );

    assert_command_succeeded(&output);
    let updated = fs::read_to_string(&config).expect("the printer config should be readable");
    assert!(
        updated.starts_with(existing),
        "existing hand-written configuration should remain unchanged:\n{updated}"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_refuses_to_replace_an_existing_name() {
    let directory = temporary_directory("duplicate-name");
    let config = directory.join("printers.toml");
    let existing = concat!(
        "[kitchen]\n",
        "transport = \"network\"\n",
        "host = \"10.42.0.20\"\n",
        "port = 9100\n",
    );
    fs::write(&config, existing).expect("the existing config should be writable");

    let output = run_non_interactive_add(
        &config,
        &["kitchen", "--transport", "network", "--host", "10.42.0.71"],
    );

    assert!(!output.status.success(), "duplicate names must fail");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("already configured"),
        "the error should explain the conflict:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&config).expect("the printer config should remain readable"),
        existing,
        "a failed add must not modify existing configuration"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_uses_the_raw_tcp_default_without_requiring_a_profile() {
    let directory = temporary_directory("network-defaults");
    let config = directory.join("printers.toml");

    let output = run_non_interactive_add(
        &config,
        &[
            "kitchen",
            "--transport",
            "network",
            "--host",
            "printer.local",
        ],
    );

    assert_command_succeeded(&output);
    let document = fs::read_to_string(&config).expect("the printer config should be readable");
    let table = toml::from_str::<toml::Table>(&document)
        .expect("the printer config should contain valid TOML");
    let printer = table["kitchen"]
        .as_table()
        .expect("the named printer should be a table");
    assert_eq!(printer["port"].as_integer(), Some(9100));
    assert!(
        !printer.contains_key("profile"),
        "a profile is optional for raw printing"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_list_reports_saved_network_reachability_without_discovery() {
    let directory = temporary_directory("network-reachability");
    let config = directory.join("printers.toml");
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("the reachable test endpoint should bind");
    let port = listener
        .local_addr()
        .expect("the test endpoint should have an address")
        .port()
        .to_string();
    let add = run_non_interactive_add(
        &config,
        &[
            "kitchen",
            "--transport",
            "network",
            "--host",
            "127.0.0.1",
            "--port",
            &port,
        ],
    );
    assert_command_succeeded(&add);

    let received = thread::spawn(move || {
        let (mut connection, _) = listener
            .accept()
            .expect("the configured endpoint should receive the probe");
        let mut bytes = Vec::new();
        connection
            .read_to_end(&mut bytes)
            .expect("the probe connection should close cleanly");
        bytes
    });
    let connected = run_printers_list(&config);
    assert_command_succeeded(&connected);
    let stdout = String::from_utf8_lossy(&connected.stdout);
    assert!(stdout.contains("] kitchen\n"));
    assert!(stdout.contains("status: connected"));
    assert!(stdout.contains("profile: unassigned"));
    assert!(stdout.contains("transport: network"));
    assert!(stdout.contains(&format!("network: 127.0.0.1:{port}")));
    assert!(
        String::from_utf8_lossy(&connected.stderr)
            .contains("Discover connected printers with: escpost printers discover"),
        "the discover tip should print even when printers were listed:\n{}",
        String::from_utf8_lossy(&connected.stderr)
    );
    assert!(
        received
            .join()
            .expect("the probe receiver should finish")
            .is_empty(),
        "reachability checks must never send printable bytes"
    );

    let unavailable = run_printers_list(&config);
    assert_command_succeeded(&unavailable);
    assert!(
        String::from_utf8_lossy(&unavailable.stdout).contains("status: unavailable"),
        "a refused connection should make the saved target unavailable:\n{}",
        String::from_utf8_lossy(&unavailable.stdout)
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_list_checks_usb_presence_without_opening_any_device() {
    // Unlike `run_printers_list`'s helper (which passes `--transport
    // network` specifically to avoid touching USB hardware), this test runs
    // the default `list` against the real `NusbInventory` on purpose: USB
    // presence must come from OS device metadata alone, so `list` must
    // never fail with a permission error opening a real, unrelated USB
    // device connected to the test machine, and a configured USB printer
    // that matches nothing actually connected must cleanly report
    // `unavailable` rather than erroring.
    let directory = temporary_directory("usb-presence-metadata-only");
    let config = directory.join("printers.toml");
    fs::write(
        &config,
        "\
[phantom-usb]
transport = \"usb\"
vendor_id = \"0x9999\"
product_id = \"0x0001\"
interface_number = 0
out_endpoint = \"0x01\"
",
    )
    .expect("a USB-only configuration should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["printers", "--config"])
        .arg(&config)
        .arg("list")
        .output()
        .expect("the escpost command should finish");

    assert_command_succeeded(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("] phantom-usb"),
        "the configured USB printer should still be listed:\n{stdout}"
    );
    assert!(
        stdout.contains("status: unavailable"),
        "no connected device matches this identity, so it must be unavailable, not an error:\n{stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("permission denied") && !stderr.contains("could not open USB device"),
        "listing must never open a USB device, including unrelated ones actually connected to this machine:\n{stderr}"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_list_reports_no_printers_configured_for_an_empty_registry() {
    let directory = temporary_directory("empty-registry");
    let config = directory.join("printers.toml");
    // `list` requires an explicit --config path to already exist (unlike
    // `discover`'s `load_for_update`), so an empty-but-valid file stands in
    // for a freshly initialized, still-empty registry.
    fs::write(&config, "").expect("an empty configuration file should be writable");

    // The network filter keeps this test from depending on USB hardware;
    // the empty configuration file makes the registry itself empty.
    let output = run_printers_list(&config);

    assert_command_succeeded(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "No printers configured.\n"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Discover connected printers with: escpost printers discover"),
        "an empty registry should still hint at discovery:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_non_interactive_reports_the_first_missing_required_value() {
    let directory = temporary_directory("non-interactive-missing-value");
    let config = directory.join("printers.toml");

    let output = run_non_interactive_add(&config, &["kitchen", "--transport", "network"]);

    assert_failed_without_configuration(&output, &config, "network printer host is required");
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_rejects_port_zero_without_writing_configuration() {
    let directory = temporary_directory("invalid-network-port");
    let config = directory.join("printers.toml");

    let output = run_non_interactive_add(
        &config,
        &[
            "kitchen",
            "--transport",
            "network",
            "--host",
            "10.42.0.71",
            "--port",
            "0",
        ],
    );

    assert_failed_without_configuration(&output, &config, "port must be between 1 and 65535");
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_rejects_a_blank_network_host() {
    let directory = temporary_directory("blank-network-host");
    let config = directory.join("printers.toml");

    let output = run_non_interactive_add(
        &config,
        &["kitchen", "--transport", "network", "--host", "   "],
    );

    assert_failed_without_configuration(&output, &config, "network printer host must not be blank");
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_rejects_a_blank_printer_name() {
    let directory = temporary_directory("blank-printer-name");
    let config = directory.join("printers.toml");

    let output = run_non_interactive_add(
        &config,
        &[" ", "--transport", "network", "--host", "10.42.0.71"],
    );

    assert_failed_without_configuration(&output, &config, "printer name must not be blank");
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_rejects_a_blank_optional_profile() {
    let directory = temporary_directory("blank-printer-profile");
    let config = directory.join("printers.toml");

    let output = run_non_interactive_add(
        &config,
        &[
            "kitchen",
            "--transport",
            "network",
            "--host",
            "10.42.0.71",
            "--profile",
            " ",
        ],
    );

    assert_failed_without_configuration(&output, &config, "printer profile must not be blank");
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_refuses_to_modify_invalid_existing_configuration() {
    let directory = temporary_directory("invalid-existing-config");
    let config = directory.join("printers.toml");
    let existing = "[broken-network]\ntransport = \"network\"\n";
    fs::write(&config, existing).expect("the existing config should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "--non-interactive",
            "printers",
            "--config",
            config.to_str().expect("the config path should be UTF-8"),
            "add",
            "kitchen",
            "--transport",
            "network",
            "--host",
            "10.42.0.71",
        ])
        .output()
        .expect("the escpost command should finish");

    assert!(
        !output.status.success(),
        "invalid existing configuration must fail"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid printer configuration"),
        "the error should identify the invalid config:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&config).expect("the printer config should remain readable"),
        existing,
        "a failed add must leave invalid input available for correction"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_creates_a_private_configuration_file_and_its_directory() {
    let directory = temporary_directory("private-config");
    let config = directory.join("nested/config/printers.toml");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "--non-interactive",
            "printers",
            "--config",
            config.to_str().expect("the config path should be UTF-8"),
            "add",
            "kitchen",
            "--transport",
            "network",
            "--host",
            "10.42.0.71",
        ])
        .output()
        .expect("the escpost command should finish");

    assert!(
        output.status.success(),
        "command failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mode = fs::metadata(&config)
        .expect("the printer config should have metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "new user configuration should be private");
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_accepts_a_configuration_filename_in_the_current_directory() {
    let directory = temporary_directory("relative-config");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "--non-interactive",
            "printers",
            "--config",
            "printers.toml",
            "add",
            "kitchen",
            "--transport",
            "network",
            "--host",
            "10.42.0.71",
        ])
        .current_dir(&directory)
        .output()
        .expect("the escpost command should finish");

    assert!(
        output.status.success(),
        "command failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        directory.join("printers.toml").is_file(),
        "the relative config should be created in the working directory"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_uses_the_platform_configuration_directory() {
    let directory = temporary_directory("add-platform-config");
    let expected = directory.join("escpost/printers.toml");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "--non-interactive",
            "printers",
            "add",
            "kitchen",
            "--transport",
            "network",
            "--host",
            "10.42.0.71",
        ])
        .env_remove("ESCPOST_CONFIG_DIR")
        .env("XDG_CONFIG_HOME", &directory)
        .output()
        .expect("the escpost command should finish");

    assert!(
        output.status.success(),
        "command failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        expected.is_file(),
        "the printer should be stored in the platform config directory"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_does_not_prompt_without_a_terminal() {
    let directory = temporary_directory("no-terminal");
    let config = directory.join("printers.toml");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "printers",
            "--config",
            config.to_str().expect("the config path should be UTF-8"),
            "add",
        ])
        .output()
        .expect("the escpost command should finish instead of waiting for input");

    assert!(!output.status.success(), "missing values must fail");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("printer name is required"),
        "the error should explain why prompting was unavailable:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !config.exists(),
        "a failed command must not create configuration"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_list_reads_an_explicit_configuration_path() {
    let directory = temporary_directory("explicit-config");
    let config = directory.join("printers.toml");
    let ignored_directory = directory.join("ignored");
    fs::create_dir(&ignored_directory).expect("the override directory should be creatable");
    fs::write(ignored_directory.join("printers.toml"), "also not TOML")
        .expect("the ignored config should be writable");
    fs::write(&config, "this is not TOML").expect("the invalid config should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["printers", "list", "--config"])
        .arg(&config)
        .env("ESCPOST_CONFIG_DIR", ignored_directory)
        .output()
        .expect("the escpost command should finish");

    assert!(!output.status.success(), "invalid configuration must fail");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(config.to_string_lossy().as_ref()),
        "the explicit path should take precedence:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_list_uses_the_escpost_configuration_directory() {
    let directory = temporary_directory("environment-config");
    let config = directory.join("printers.toml");
    fs::write(&config, "this is not TOML").expect("the invalid config should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["printers", "list"])
        .env("ESCPOST_CONFIG_DIR", &directory)
        .output()
        .expect("the escpost command should finish");

    assert!(!output.status.success(), "invalid configuration must fail");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(config.to_string_lossy().as_ref()),
        "the error should identify the resolved config:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_list_uses_the_platform_configuration_directory() {
    let directory = temporary_directory("platform-config");
    let config_directory = directory.join("escpost");
    fs::create_dir(&config_directory).expect("the app config directory should be creatable");
    let config = config_directory.join("printers.toml");
    fs::write(&config, "this is not TOML").expect("the invalid config should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["printers", "list"])
        .env_remove("ESCPOST_CONFIG_DIR")
        .env("XDG_CONFIG_HOME", &directory)
        .output()
        .expect("the escpost command should finish");

    assert!(!output.status.success(), "invalid configuration must fail");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(config.to_string_lossy().as_ref()),
        "the error should identify the platform config:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_list_does_not_create_missing_configuration() {
    let directory = temporary_directory("read-only-config");
    let application_directory = directory.join("escpost");

    let _output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["printers", "list"])
        .env_remove("ESCPOST_CONFIG_DIR")
        .env("XDG_CONFIG_HOME", &directory)
        .output()
        .expect("the escpost command should finish");

    assert!(
        !application_directory.exists(),
        "read-only listing must not create its config directory"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(target_os = "linux")]
#[test]
fn printers_grant_usb_permissions_documents_itself() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["printers", "grant-usb-permissions", "--help"])
        .output()
        .expect("the escpost command should finish");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "command failed:\n{stdout}");
    assert!(stdout.contains("Grant the current user access to USB printers"));
    assert!(stdout.contains("Usage: escpost printers grant-usb-permissions"));
}

#[cfg(target_os = "linux")]
#[test]
fn printers_grant_usb_permissions_without_root_fails_with_the_two_options() {
    // This test must not run as root: the whole point is exercising the
    // without-root failure branch, never the branch that writes to
    // /etc/udev/rules.d or shells out to udevadm.
    assert_ne!(
        current_effective_uid(),
        0,
        "this test must not run as root; it only covers the without-root failure"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["printers", "grant-usb-permissions"])
        .output()
        .expect("the escpost command should finish");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Was asked to grant access and cannot: that is a failure, not just
    // an FYI, so this must not exit successfully.
    assert!(
        !output.status.success(),
        "without root this must fail, not merely inform:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "the exit code should be 1:\nstderr:\n{stderr}"
    );
    assert_eq!(
        stdout, "",
        "nothing should be written to stdout on this failure path:\n{stdout}"
    );
    // Exact match, not just substring checks: this is the entire error
    // path's output, and its shape (the `error: ` prefix `lib.rs` adds,
    // both options, the sudo-prefixed bare-metal commands, the flush-left
    // heredoc body) is exactly what a user is expected to read or paste.
    assert_eq!(
        stderr,
        "\
error: granting USB printer access requires root

Let escpost apply it:
  sudo escpost printers grant-usb-permissions

Or run the commands yourself:
  sudo tee /etc/udev/rules.d/70-escpost-usb-printers.rules <<'EOF'
# Grant locally logged-in users access to USB printer-class devices (escpost).
SUBSYSTEM==\"usb\", ENV{ID_USB_INTERFACES}==\"*:0701*:*\", TAG+=\"uaccess\"
EOF
  sudo udevadm control --reload
  sudo udevadm trigger --subsystem-match=usb
"
    );
}

/// The test binary's own effective UID, read via `/proc/self/status` rather
/// than linking `libc`/`rustix` into the integration test crate just to
/// assert this one precondition; the workspace forbids `unsafe_code`
/// entirely, so this stays a plain filesystem read.
#[cfg(target_os = "linux")]
fn current_effective_uid() -> u32 {
    let status = fs::read_to_string("/proc/self/status")
        .expect("/proc/self/status should be readable on Linux");
    let line = status
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .expect("/proc/self/status should report Uid");
    line.split_whitespace()
        .nth(2)
        .expect("the Uid line should have an effective UID field")
        .parse()
        .expect("the effective UID should be numeric")
}

#[cfg(unix)]
fn run_non_interactive_add(config: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["--non-interactive", "printers", "--config"])
        .arg(config)
        .arg("add")
        .args(arguments)
        .output()
        .expect("the escpost command should finish")
}

#[cfg(unix)]
fn run_printers_list(config: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["printers", "--config"])
        .arg(config)
        .args(["list", "--transport", "network"])
        .output()
        .expect("the escpost command should finish")
}

#[cfg(unix)]
fn assert_command_succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "command failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
fn assert_failed_without_configuration(output: &Output, config: &Path, message: &str) {
    assert!(!output.status.success(), "an invalid command must fail");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "the error should explain the invalid value:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !config.exists(),
        "an invalid command must not create configuration"
    );
}

#[cfg(unix)]
fn temporary_directory(case: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock should be after the Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "escpost-printers-{case}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&path).expect("the test directory should be creatable");
    path
}

#[test]
fn printers_discover_documents_its_options() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["printers", "discover", "--help"])
        .output()
        .expect("the escpost command should finish");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "command failed:\n{stdout}");
    assert!(stdout.contains(
        "Find connected USB printers and network printers listening on the RAW TCP port"
    ));
    assert!(stdout.contains("Usage: escpost printers discover"));
    assert!(stdout.contains("--transport <TRANSPORT>"));
    assert!(stdout.contains("--port <PORT>"));
    assert!(stdout.contains("--subnet <CIDR>"));
    assert!(stdout.contains("--timeout <MS>"));
}

#[cfg(unix)]
#[test]
fn printers_discover_finds_a_listening_loopback_printer() {
    // 127.0.0.2 rather than 127.0.0.1: the whole 127.0.0.0/8 block routes to
    // loopback, but only 127.0.0.1 is this machine's own address, so an
    // explicit /32 on 127.0.0.2 is not self-excluded and this stand-in
    // "printer" stays discoverable.
    let listener = TcpListener::bind("127.0.0.2:0").expect("an ephemeral port should bind");
    let port = listener
        .local_addr()
        .expect("the listener should report its address")
        .port();
    let directory = temporary_directory("discover-hit");
    let config = directory.join("printers.toml");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["--non-interactive", "printers", "--config"])
        .arg(&config)
        .args([
            "discover",
            "--transport",
            "network",
            "--subnet",
            "127.0.0.2/32",
            "--port",
            &port.to_string(),
        ])
        .output()
        .expect("the escpost command should finish");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "command failed:\n{stdout}");
    assert!(stdout.contains(&format!("[1] 127.0.0.2:{port}")));
    assert!(stdout.contains("status: new"));
    assert!(
        stderr.contains(&format!("Scanning 1 network on port {port} (1 address):")),
        "stderr should announce the scanned network count and probe count:\n{stderr}"
    );
    assert!(
        stderr.contains("  - 127.0.0.2/32"),
        "stderr should list the scanned network:\n{stderr}"
    );
    assert!(
        !stderr.contains("Tip:"),
        "an explicit --subnet should not print the auto-detection tip:\n{stderr}"
    );
    assert!(
        stderr.contains(&format!(
            "Register a new network printer with: escpost printers add <NAME> --transport network --discover --port {port}"
        )),
        "stderr should hint at registering the new printer:\n{stderr}"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

/// Results are printed as they answer, so two hosts can be reported in either
/// order from one run to the next. What the command still guarantees is that
/// every host appears exactly once and that the `[N]` numbers run 1, 2, … over
/// the results actually printed — so this asserts the set and the numbering
/// rather than pinning a sequence the program no longer promises.
#[cfg(unix)]
#[test]
fn printers_discover_streams_each_answering_host_exactly_once() {
    let (_first, _second, port) = loopback_pair_on_one_port();
    let directory = temporary_directory("discover-two-hosts");
    let config = directory.join("printers.toml");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["--non-interactive", "printers", "--config"])
        .arg(&config)
        .args([
            "discover",
            "--transport",
            "network",
            "--subnet",
            "127.0.0.2/32",
            "--subnet",
            "127.0.0.3/32",
            "--port",
            &port.to_string(),
        ])
        .output()
        .expect("the escpost command should finish");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "command failed:\n{stdout}");
    for address in ["127.0.0.2", "127.0.0.3"] {
        let endpoint = format!("    network: {address}:{port}\n");
        assert_eq!(
            stdout.matches(&endpoint).count(),
            1,
            "{address} should be reported exactly once:\n{stdout}"
        );
    }
    assert_eq!(
        stdout.matches("status: new").count(),
        2,
        "both hosts should be reported as new:\n{stdout}"
    );
    for number in ["[1] ", "[2] "] {
        assert_eq!(
            stdout.matches(number).count(),
            1,
            "streamed results should be numbered continuously:\n{stdout}"
        );
    }
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

/// A listener on every local address, so a sweep of the loopback network
/// finds a host at each address it probes. It answers only the first probes:
/// its accept queue holds a fixed number of connections, and nothing takes
/// them off the queue. The kernel then drops the connection request of every
/// later probe, and each of those probes waits for its full timeout. A sweep
/// of 65533 loopback addresses set up this way reports its first results at
/// once and cannot end for hours, which is the state these tests need. It
/// also needs no network outside the machine, so it behaves the same way
/// wherever the tests run.
#[cfg(unix)]
fn stalling_loopback_listener() -> (TcpListener, u16) {
    let listener = TcpListener::bind("0.0.0.0:0").expect("an ephemeral port should bind");
    let port = listener
        .local_addr()
        .expect("the listener should report its address")
        .port();
    (listener, port)
}

/// Start a sweep that cannot end on its own. See `stalling_loopback_listener`
/// for why every address answers or stalls.
#[cfg(unix)]
fn spawn_stalling_sweep(config: &Path, port: u16) -> Child {
    Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["--non-interactive", "printers", "--config"])
        .arg(config)
        .args([
            "discover",
            "--transport",
            "network",
            "--subnet",
            "127.0.0.0/16",
            "--port",
            &port.to_string(),
            "--timeout",
            "30000",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the escpost command should start")
}

/// Read streamed results until one whole result block is in hand. Returns
/// everything read and the endpoint that block reports.
#[cfg(unix)]
fn read_first_result(results: &mut impl BufRead) -> (String, String) {
    let mut streamed = String::new();
    loop {
        let mut line = String::new();
        let read = results
            .read_line(&mut line)
            .expect("the streamed results should be readable");
        assert!(
            read > 0,
            "the sweep ended before streaming its first result:\n{streamed}"
        );
        streamed.push_str(&line);
        // The endpoint line comes after the heading, so reaching it proves a
        // whole result reached the reader while the sweep still probes.
        if let Some(endpoint) = line.strip_prefix("    network: ") {
            let endpoint = endpoint.trim_end().to_owned();
            return (streamed, endpoint);
        }
    }
}

/// The point of streaming: a sweep stopped with Ctrl+C keeps every result it
/// already printed, still tells the user how to register it, and still reports
/// an interrupt to whatever started it.
#[cfg(unix)]
#[test]
fn printers_discover_keeps_its_results_when_the_sweep_is_interrupted() {
    let (_listener, port) = stalling_loopback_listener();
    let directory = temporary_directory("discover-interrupt");
    let config = directory.join("printers.toml");
    let mut child = spawn_stalling_sweep(&config, port);

    let mut results = BufReader::new(child.stdout.take().expect("stdout should be piped"));
    let (mut streamed, endpoint) = read_first_result(&mut results);

    // The sweep cannot end on its own, so the signal always reaches a running
    // command. It is sent again every 100 ms until the command ends, so one
    // signal that gets lost cannot make the test wait for ever. `sh`'s
    // builtin `kill` sends it, so the test needs no libc binding of its own.
    let interrupting = Arc::new(AtomicBool::new(true));
    let signals = Arc::clone(&interrupting);
    let identifier = child.id();
    let interrupter = thread::spawn(move || {
        while signals.load(Ordering::Relaxed) {
            let sent = Command::new("sh")
                .args(["-c", &format!("kill -INT {identifier}")])
                .status()
                .expect("the interrupter should run");
            assert!(sent.success(), "the interrupt should be delivered");
            thread::sleep(Duration::from_millis(100));
        }
    });

    // Read to the end first: that finishes when the command closes stdout,
    // which it does as it exits. The process identifier stays reserved until
    // the `wait` below, so no signal can reach another process.
    results
        .read_to_string(&mut streamed)
        .expect("the remaining results should be readable");
    interrupting.store(false, Ordering::Relaxed);
    interrupter.join().expect("the interrupter should finish");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr should be piped")
        .read_to_string(&mut stderr)
        .expect("the diagnostics should be readable");
    let status = child.wait().expect("the escpost command should finish");

    assert_eq!(
        status.code(),
        Some(130),
        "an interrupted sweep must still report the interrupt (128 + SIGINT):\n{stderr}"
    );
    assert!(
        streamed.contains(&format!("[1] {endpoint}\n")),
        "the printed result must survive the interrupt intact:\n{streamed}"
    );
    assert!(
        streamed.contains(&format!("    network: {endpoint}\n")),
        "the printed result must survive the interrupt intact:\n{streamed}"
    );
    assert!(
        !streamed.contains("No printers discovered."),
        "an interrupted sweep must not claim it found nothing:\n{streamed}"
    );
    assert!(
        stderr.contains(&format!(
            "Register a new network printer with: escpost printers add <NAME> --transport network --discover --port {port}"
        )),
        "an interrupted sweep should still name the command that registers what it found:\n{stderr}"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

/// `printers discover | head -n 1` closes the pipe as soon as it has read
/// enough. The sweep must stop there and the command must end quietly,
/// instead of probing every address that is left and then reporting a broken
/// pipe as a failure.
#[cfg(unix)]
#[test]
fn printers_discover_stops_quietly_when_its_reader_closes_the_pipe() {
    let (listener, port) = stalling_loopback_listener();
    // One accepted connection every 20 ms makes one place in the queue free,
    // so a probe that waits gets an answer and a new result goes to the
    // reader. Without this the sweep has nothing more to write and never
    // learns that the pipe is closed.
    let queue = listener
        .try_clone()
        .expect("the listener should be clonable");
    thread::spawn(move || {
        while queue.accept().is_ok() {
            thread::sleep(Duration::from_millis(20));
        }
    });
    let directory = temporary_directory("discover-broken-pipe");
    let config = directory.join("printers.toml");
    let mut child = spawn_stalling_sweep(&config, port);

    let mut results = BufReader::new(child.stdout.take().expect("stdout should be piped"));
    let (_streamed, _endpoint) = read_first_result(&mut results);
    // What `head` does once it has its line.
    drop(results);

    // The sweep would run for hours if it kept probing, so a command that is
    // still alive after 30 seconds did not stop at the closed pipe.
    let status = wait_for_exit(
        &mut child,
        Duration::from_secs(30),
        "the sweep should stop as soon as its reader closes the pipe",
    );
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr should be piped")
        .read_to_string(&mut stderr)
        .expect("the diagnostics should be readable");

    assert!(
        status.success(),
        "a reader that closes the pipe is not a failure:\n{stderr}"
    );
    assert!(
        !stderr.contains("Broken pipe"),
        "a closed pipe must not be reported as an error:\n{stderr}"
    );
    assert!(
        !stderr.contains("error:"),
        "a closed pipe must not be reported as an error:\n{stderr}"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

/// Wait for a command to end, but not longer than `limit`. A command that is
/// still alive then is killed and `reason` is reported, so a failed test
/// leaves no process behind.
#[cfg(unix)]
fn wait_for_exit(child: &mut Child, limit: Duration, reason: &str) -> ExitStatus {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child
            .try_wait()
            .expect("the escpost command should be waitable")
        {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{reason}");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Two loopback addresses listening on one port, so a single sweep has two
/// hosts to report. The port is chosen by binding the first address and then
/// claiming the same number on the second, retrying until one is free on both.
#[cfg(unix)]
fn loopback_pair_on_one_port() -> (TcpListener, TcpListener, u16) {
    for _ in 0..16 {
        let first = TcpListener::bind("127.0.0.2:0").expect("an ephemeral port should bind");
        let port = first
            .local_addr()
            .expect("the listener should report its address")
            .port();
        if let Ok(second) = TcpListener::bind(("127.0.0.3", port)) {
            return (first, second, port);
        }
    }
    panic!("no ephemeral port was free on both loopback addresses");
}

#[test]
fn printers_discover_announces_how_many_addresses_it_will_probe() {
    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args([
            "printers",
            "discover",
            "--transport",
            "network",
            "--subnet",
            // A loopback /30, so the two probes stay on this machine. The
            // whole 127.0.0.0/8 block routes to loopback and a probe to an
            // address with no listener is refused at once. A reserved
            // documentation range is not a safe substitute: a route or a VPN
            // can put a real host behind one, and this suite has seen both
            // TEST-NET-1 and TEST-NET-3 answer.
            //
            // The /30 starts at .4 and not at .0, because 127.0.0.1 is this
            // machine's own address. Self-exclusion removes it from any
            // sweep that covers it, which would make the count one, not two.
            "127.0.0.4/30",
            "--timeout",
            "1",
        ])
        .output()
        .expect("the escpost command should finish");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "command failed:\n{stderr}");
    assert!(stderr.contains("Scanning 1 network on port 9100 (2 addresses):"));
}

#[cfg(unix)]
#[test]
fn printers_discover_reports_an_empty_sweep() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port should bind");
    let port = listener
        .local_addr()
        .expect("the listener should report its address")
        .port();
    drop(listener);
    let directory = temporary_directory("discover-miss");
    let config = directory.join("printers.toml");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["--non-interactive", "printers", "--config"])
        .arg(&config)
        .args([
            "discover",
            "--transport",
            "network",
            "--subnet",
            "127.0.0.1/32",
            "--port",
            &port.to_string(),
        ])
        .output()
        .expect("the escpost command should finish");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "command failed:\n{stdout}");
    assert!(stdout.contains("No printers discovered."));
    assert!(
        !stderr.contains("Register"),
        "an empty sweep should not hint at registering a printer:\n{stderr}"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_discover_rejects_network_scan_options_with_usb_transport() {
    let directory = temporary_directory("discover-usb-with-network-options");
    let config = directory.join("printers.toml");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["--non-interactive", "printers", "--config"])
        .arg(&config)
        .args(["discover", "--transport", "usb", "--subnet", "127.0.0.1/32"])
        .output()
        .expect("the escpost command should finish");

    assert!(!output.status.success(), "invalid discovery must fail");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: --subnet, --port, and --timeout are only valid when discovering network printers\n",
        "invalid discovery stderr must contain only the error"
    );
    assert!(
        !config.exists(),
        "invalid discovery must not create configuration"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_discover_validates_zero_port_before_reading_configuration() {
    let directory = temporary_directory("discover-zero-before-config");
    let config = directory.join("printers.toml");
    fs::write(&config, "this is not valid TOML").expect("the invalid fixture should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["--non-interactive", "printers", "--config"])
        .arg(&config)
        .args([
            "discover",
            "--transport",
            "network",
            "--subnet",
            "127.0.0.1/32",
            "--port",
            "0",
        ])
        .output()
        .expect("the escpost command should finish");

    assert!(!output.status.success(), "zero port discovery must fail");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: printer port must be between 1 and 65535\n",
        "argument validation must win without a configuration notice or configuration error"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_discover_refuses_a_subnet_too_large_to_scan() {
    let directory = temporary_directory("discover-unbounded-subnet");
    let config = directory.join("printers.toml");

    let output = Command::new(env!("CARGO_BIN_EXE_escpost"))
        .args(["--non-interactive", "printers", "--config"])
        .arg(&config)
        .args([
            "discover",
            "--transport",
            "network",
            "--subnet",
            "0.0.0.0/0",
        ])
        .output()
        .expect("the escpost command should finish");

    assert!(
        !output.status.success(),
        "a subnet of four billion addresses must fail"
    );
    // The same refusal, in the same words, that the HTTP endpoint turns into
    // a 400: the limit lives in the shared discovery layer so the terminal
    // and the browser can never disagree about what is scannable.
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: subnet 0.0.0.0/0 is too large to scan (at most /16)\n",
        "the refusal must arrive before any scan announcement"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_discover_registers_the_single_discovered_printer() {
    // 127.0.0.2 rather than 127.0.0.1: the whole 127.0.0.0/8 block routes to
    // loopback, but only 127.0.0.1 is this machine's own address, so an
    // explicit /32 on 127.0.0.2 is not self-excluded and this stand-in
    // "printer" stays discoverable.
    let listener = TcpListener::bind("127.0.0.2:0").expect("an ephemeral port should bind");
    let port = listener
        .local_addr()
        .expect("the listener should report its address")
        .port();
    let directory = temporary_directory("add-discover");
    let config = directory.join("printers.toml");

    let output = run_non_interactive_add(
        &config,
        &[
            "kitchen",
            "--transport",
            "network",
            "--discover",
            "--subnet",
            "127.0.0.2/32",
            "--port",
            &port.to_string(),
        ],
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "command failed:\n{stderr}");
    let document =
        fs::read_to_string(&config).expect("the printer configuration should be readable");
    let table = toml::from_str::<toml::Table>(&document).expect("the configuration should be TOML");
    let printer = table["kitchen"]
        .as_table()
        .expect("the configured printer should be a table");
    assert_eq!(printer["transport"].as_str(), Some("network"));
    assert_eq!(printer["host"].as_str(), Some("127.0.0.2"));
    assert_eq!(printer["port"].as_integer(), Some(i64::from(port)));
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_discover_fails_when_nothing_is_listening() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port should bind");
    let port = listener
        .local_addr()
        .expect("the listener should report its address")
        .port();
    drop(listener);
    let directory = temporary_directory("add-discover-miss");
    let config = directory.join("printers.toml");

    let output = run_non_interactive_add(
        &config,
        &[
            "kitchen",
            "--transport",
            "network",
            "--discover",
            "--subnet",
            "127.0.0.1/32",
            "--port",
            &port.to_string(),
        ],
    );

    assert_failed_without_configuration(&output, &config, "no printer is listening on port");
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_discover_rejects_an_explicit_host() {
    let directory = temporary_directory("add-discover-host");
    let config = directory.join("printers.toml");

    let output = run_non_interactive_add(
        &config,
        &[
            "kitchen",
            "--transport",
            "network",
            "--discover",
            "--host",
            "10.42.0.71",
        ],
    );

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "clap should reject --discover with --host:\n{stderr}"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_discover_rejects_usb_selectors() {
    let directory = temporary_directory("add-discover-usb-selectors");
    let config = directory.join("printers.toml");

    let output = run_non_interactive_add(
        &config,
        &[
            "kitchen",
            "--discover",
            "--vendor-id",
            "0x0416",
            "--product-id",
            "0x5011",
        ],
    );

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "clap should reject --discover with USB selectors:\n{stderr}"
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}

#[cfg(unix)]
#[test]
fn printers_add_discover_rejects_usb_transport() {
    let directory = temporary_directory("add-discover-usb");
    let config = directory.join("printers.toml");

    let output = run_non_interactive_add(&config, &["counter", "--transport", "usb", "--discover"]);

    assert_failed_without_configuration(
        &output,
        &config,
        "--discover is only valid for network printers",
    );
    fs::remove_dir_all(directory).expect("the test directory should be removable");
}
