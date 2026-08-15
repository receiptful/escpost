use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::features::printers::cli::PrintersArgs;
use crate::features::profiles::cli::ProfilesArgs;

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

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub(crate) enum InputFormat {
    #[default]
    Auto,
    Binary,
    Hex,
}
