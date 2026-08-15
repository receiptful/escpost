use clap::{Parser, Subcommand};

use crate::features::capture::cli::ServeArgs;
use crate::features::printers::cli::PrintersArgs;
use crate::features::printing::cli::PrintArgs;
use crate::features::profiles::cli::ProfilesArgs;
use crate::features::rendering::cli::RenderArgs;

pub(crate) mod web;

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
