use std::net::{Ipv4Addr, SocketAddr};

use clap::{Parser, Subcommand};

use crate::features::capture::cli::ServeArgs;
use crate::features::printers::cli::PrintersArgs;
use crate::features::printing::cli::PrintArgs;
use crate::features::profiles::cli::ProfilesArgs;
use crate::features::rendering::cli::RenderArgs;

pub(crate) mod web;

pub(crate) fn parse_listener_address(value: &str) -> Result<SocketAddr, String> {
    if let Ok(port) = value.parse::<u16>() {
        return Ok(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
    }
    value
        .parse()
        .map_err(|_| "expected a port or an IP address with a port".to_owned())
}

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

    /// Send a known ESC/POS byte stream unchanged to a printer.
    Print(PrintArgs),

    /// Capture RAW TCP print jobs and preview them in the web app.
    Serve(ServeArgs),

    /// List available printers and manage discovery or pairing.
    Printers(PrintersArgs),

    /// Browse the embedded catalog of supported printer profiles.
    Profiles(ProfilesArgs),
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    use clap::Parser;

    use super::{Cli, Command};

    #[test]
    fn bare_listener_ports_resolve_to_ipv4_loopback() {
        let expected_raw = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9101));
        let expected_web = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9001));

        let serve = Cli::try_parse_from([
            "escpost",
            "serve",
            "--listen",
            "9101",
            "--web-listen",
            "9001",
        ])
        .expect("serve should accept bare listener ports");
        let Command::Serve(serve) = serve.command else {
            panic!("serve arguments should parse as the serve command");
        };
        assert_eq!(serve.listen, Some(Some(expected_raw)));
        assert_eq!(serve.web_listen, Some(Some(expected_web)));
    }
}
