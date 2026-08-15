use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::discovery::Subnet;

#[derive(Debug, Parser)]
#[command(
    name = "escpost",
    version,
    about = "The ESC/POS Tools and Workbench",
    subcommand_required = true,
    arg_required_else_help = true
)]
pub(crate) struct Cli {
    /// Never prompt for missing values.
    #[arg(long, global = true)]
    pub(crate) non_interactive: bool,

    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Render a known ESC/POS byte stream.
    Render(RenderArgs),

    /// Send a known ESC/POS byte stream unchanged to a configured printer.
    Print(PrintArgs),

    /// Capture RAW TCP print jobs and preview them in the web viewer.
    Serve(ServeArgs),

    /// List available printers and manage discovery or pairing.
    Printers(PrintersArgs),

    /// Browse the embedded catalog of supported printer profiles.
    Profiles(ProfilesArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ServeArgs {
    /// Printer profile used to render captured jobs.
    #[arg(long, default_value = "REFERENCE")]
    pub(crate) profile: String,

    /// Address for the RAW TCP printer. When omitted, the first free loopback
    /// port from 9100 through 9109 is used.
    #[arg(long)]
    pub(crate) listen: Option<SocketAddr>,

    /// Address for the web viewer. When omitted, the first free loopback port
    /// from 9000 through 9099 is used.
    #[arg(long)]
    pub(crate) web_listen: Option<SocketAddr>,

    /// Complete a held-open connection's job after this many seconds of silence.
    /// Use 0 to disable and end a job only when the connection closes.
    #[arg(long, value_name = "SECONDS", default_value_t = 20.0)]
    pub(crate) idle_timeout: f64,

    /// Preview pixel density: subpixels per dot. 1 is dot resolution; N renders
    /// at N× density.
    #[arg(long, value_name = "N", default_value_t = 3)]
    pub(crate) scale: u32,

    /// Anti-alias glyph edges into a grayscale preview (cosmetic; never what a
    /// printer emits). Pass --antialias=false for faithful 1-bit dots.
    #[arg(long, num_args = 0..=1, default_value_t = true, default_missing_value = "true")]
    pub(crate) antialias: bool,

    /// Do not open the web viewer in the default browser on startup. Auto-open
    /// is also skipped with --non-interactive, without a terminal, or when the
    /// BROWSER=none or CI environment variables are set.
    #[arg(long, alias = "no-browser")]
    pub(crate) no_open: bool,
}

#[derive(Debug, Args)]
pub(crate) struct RenderArgs {
    /// Raw ESC/POS file, hexadecimal file, case directory, or - for stdin.
    pub(crate) source: PathBuf,

    /// Input representation.
    #[arg(long, value_enum, default_value_t = InputFormat::Auto)]
    pub(crate) format: InputFormat,

    /// Printer profile used to interpret the input.
    #[arg(long)]
    pub(crate) profile: Option<String>,

    /// Write one PNG to this path, or use - for stdout.
    #[arg(short = 'o', long = "output", conflicts_with = "output_dir")]
    pub(crate) output: Option<PathBuf>,

    /// Write every rendered sheet and a manifest to this directory.
    #[arg(long, conflicts_with = "output")]
    pub(crate) output_dir: Option<PathBuf>,

    /// Select one one-based sheet for single-PNG output.
    #[arg(long, conflicts_with = "output_dir", requires = "output")]
    pub(crate) sheet: Option<usize>,

    /// Start the local web viewer and keep running.
    #[arg(long)]
    pub(crate) web: bool,

    /// Start the web viewer and open it in the default browser.
    #[arg(long)]
    pub(crate) browser: bool,

    /// Exact address for the web viewer.
    #[arg(long)]
    pub(crate) web_listen: Option<SocketAddr>,

    /// Rerender a filesystem source whenever it changes.
    #[arg(long)]
    pub(crate) watch: bool,

    /// Output pixel density: subpixels per dot. 1 is dot resolution; N renders
    /// at N× density.
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) scale: u32,

    /// Anti-alias glyph edges into a grayscale preview (cosmetic; never what a
    /// printer emits). Pass --antialias for a nicer on-screen render.
    #[arg(long, num_args = 0..=1, default_value_t = false, default_missing_value = "true")]
    pub(crate) antialias: bool,
}

#[derive(Debug, Args)]
pub(crate) struct PrintArgs {
    /// Raw ESC/POS file, hexadecimal file, case directory, or - for stdin.
    pub(crate) source: PathBuf,

    /// Input representation.
    #[arg(long, value_enum, default_value_t = InputFormat::Auto)]
    pub(crate) format: InputFormat,

    /// Configured printer name.
    #[arg(long)]
    pub(crate) printer: Option<String>,

    /// Read printer configuration from this exact file.
    #[arg(long, value_name = "FILE")]
    pub(crate) config: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct PrintersArgs {
    /// Read printer configuration from this exact file.
    #[arg(long, global = true, value_name = "FILE")]
    pub(crate) config: Option<PathBuf>,

    #[command(subcommand)]
    pub(crate) command: PrintersCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum PrintersCommand {
    /// List currently usable printers.
    List(ListPrintersArgs),

    /// Register a printer in the local configuration.
    Add(AddPrinterArgs),

    /// Find connected USB printers and network printers listening on the RAW TCP port.
    Discover(DiscoverPrintersArgs),

    /// Grant the current user access to USB printers (writes a udev rule; run with sudo).
    #[cfg(target_os = "linux")]
    GrantUsbPermissions(GrantUsbPermissionsArgs),
}

#[cfg(target_os = "linux")]
#[derive(Debug, Args)]
pub(crate) struct GrantUsbPermissionsArgs {}

#[derive(Debug, Args)]
pub(crate) struct ListPrintersArgs {
    /// Show only one connection transport.
    #[arg(long, value_enum)]
    pub(crate) transport: Option<InventoryTransport>,
}

#[derive(Debug, Args)]
pub(crate) struct DiscoverPrintersArgs {
    /// Discover only one connection transport.
    #[arg(long, value_enum)]
    pub(crate) transport: Option<InventoryTransport>,

    /// Raw TCP port to probe. Defaults to 9100.
    #[arg(long)]
    pub(crate) port: Option<u16>,

    /// Scan this network (CIDR notation, for example 10.42.0.0/24) instead
    /// of the directly connected networks. May be repeated.
    #[arg(long, value_name = "CIDR", value_parser = Subnet::parse)]
    pub(crate) subnet: Vec<Subnet>,

    /// Per-host connection timeout in milliseconds. Defaults to 1000.
    #[arg(long, value_name = "MS")]
    pub(crate) timeout: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum InventoryTransport {
    Usb,
    Network,
}

#[derive(Debug, Args)]
pub(crate) struct AddPrinterArgs {
    /// Developer-assigned printer name.
    pub(crate) name: Option<String>,

    /// Connection transport.
    #[arg(long, value_enum)]
    pub(crate) transport: Option<PrinterTransport>,

    /// Network hostname or IP address.
    #[arg(long)]
    pub(crate) host: Option<String>,

    /// Raw TCP port. Defaults to 9100.
    #[arg(long)]
    pub(crate) port: Option<u16>,

    /// Select a USB printer by vendor ID (decimal or `0x`-prefixed hexadecimal).
    #[arg(long, value_parser = parse_usb_id)]
    pub(crate) vendor_id: Option<u16>,

    /// Select a USB printer by product ID (decimal or `0x`-prefixed hexadecimal).
    #[arg(long, value_parser = parse_usb_id)]
    pub(crate) product_id: Option<u16>,

    /// Select a USB printer by exact serial number.
    #[arg(long)]
    pub(crate) serial: Option<String>,

    /// Optional rendering profile.
    #[arg(long)]
    pub(crate) profile: Option<String>,

    /// Discover listening network printers and register the chosen one
    /// instead of passing --host.
    #[arg(
        long,
        conflicts_with_all = ["host", "vendor_id", "product_id", "serial"]
    )]
    pub(crate) discover: bool,

    /// Scan this network (CIDR notation, for example 10.42.0.0/24) instead
    /// of the directly connected networks. May be repeated.
    #[arg(long, value_name = "CIDR", value_parser = Subnet::parse, requires = "discover")]
    pub(crate) subnet: Vec<Subnet>,

    /// Per-host connection timeout in milliseconds during discovery.
    #[arg(long, value_name = "MS", requires = "discover")]
    pub(crate) timeout: Option<u64>,
}

/// Parse a USB vendor or product identifier given in decimal or `0x`-prefixed
/// hexadecimal, matching how the same identifiers are stored in `printers.toml`.
fn parse_usb_id(value: &str) -> Result<u16, String> {
    let text = value.trim();
    let parsed = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hexadecimal) => u16::from_str_radix(hexadecimal, 16),
        None => text.parse::<u16>(),
    };
    parsed.map_err(|_| {
        format!("expected a decimal or 0x-prefixed 16-bit USB identifier, found `{value}`")
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum PrinterTransport {
    Usb,
    Network,
}

#[derive(Debug, Args)]
pub(crate) struct ProfilesArgs {
    #[command(subcommand)]
    pub(crate) command: ProfilesCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProfilesCommand {
    /// List available printer profiles.
    List(ListProfilesArgs),
    /// Show the full details of a single printer profile.
    Show(ShowProfileArgs),
    /// Interactively pick a profile and print its id.
    Find(FindProfileArgs),
}

#[derive(Debug, Args)]
pub(crate) struct FindProfileArgs {}

#[derive(Debug, Args)]
pub(crate) struct ListProfilesArgs {
    /// Show only profiles from this vendor (case-insensitive substring).
    #[arg(long)]
    pub(crate) vendor: Option<String>,

    /// Show only profiles with this calibration provenance.
    #[arg(long, value_enum)]
    pub(crate) source: Option<SourceFilter>,

    /// Show only profiles whose id, vendor, or model contains this text
    /// (case-insensitive).
    #[arg(long)]
    pub(crate) search: Option<String>,

    /// Print the full profile catalog as JSON instead of a table.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ShowProfileArgs {
    /// Profile id (as passed to --profile).
    pub(crate) id: String,

    /// Print the profile as JSON instead of the detail view.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum SourceFilter {
    Calibrated,
    Synthesized,
    Virtual,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub(crate) enum InputFormat {
    #[default]
    Auto,
    Binary,
    Hex,
}
