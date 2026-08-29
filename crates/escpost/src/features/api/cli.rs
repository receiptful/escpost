//! Terminal adapter for the local REST API.

use std::net::SocketAddr;

use clap::Args;

use crate::error::CliError;
use crate::net;

/// Port 9180 begins a range deliberately clear of the two this binary already
/// uses: 9000–9099 for the preview viewer and 9100–9109 for the RAW TCP
/// printer. 9100 is also the conventional JetDirect port, so an HTTP client
/// aimed there would deliver a malformed request to a raw print path (D3).
const FIRST_API_PORT: u16 = 9180;
const LAST_API_PORT: u16 = 9189;

#[derive(Debug, Args)]
pub(crate) struct ApiArgs {
    /// Address for the REST API. When omitted, the first free loopback port
    /// from 9180 through 9189 is used.
    #[arg(long)]
    pub(crate) listen: Option<SocketAddr>,

    /// Read printer configuration from this exact file.
    #[arg(long, value_name = "FILE")]
    pub(crate) config: Option<std::path::PathBuf>,

    /// Accept requests only from this Chrome extension id. By default any
    /// extension is accepted, because a reinstall changes the id and pinning
    /// the wrong one locks the extension out with no recovery.
    #[arg(long, value_name = "ID")]
    pub(crate) extension_id: Option<String>,
}

pub(crate) async fn run(arguments: ApiArgs, _non_interactive: bool) -> Result<(), CliError> {
    let listener = net::bind_loopback(arguments.listen, FIRST_API_PORT..=LAST_API_PORT)
        .await
        .map_err(|failure| match failure {
            net::BindFailure::Address { address, source } => CliError::BindApi { address, source },
            net::BindFailure::RangeExhausted => CliError::NoAutomaticApiPort,
        })?;
    let address = listener.local_addr().map_err(CliError::ServeApi)?;

    // D1. bind_loopback only ever binds 127.0.0.1, so this cannot currently
    // fire; it is here so that a future change which widens the bind is caught
    // by a human reading the terminal rather than by a customer.
    if !address.ip().is_loopback() {
        return Err(CliError::ApiNotLoopback(address));
    }

    eprintln!("escpost api: http://{address}");
    eprintln!("Press Ctrl+C to stop.");

    let state = super::ApiState {
        extension_id: arguments.extension_id,
        config: arguments.config,
        ..Default::default()
    };
    warn_about_unresolvable_profiles(&state);

    super::http::serve(listener, state)
        .await
        .map_err(CliError::ServeApi)
}

/// Name any printer whose configured profile is not in the catalog.
///
/// `/printers` reports such a profile as absent rather than advertising one
/// that does not exist, which is correct but silent. An operator should learn
/// about a typo here, at startup, instead of from a print that fails days
/// later.
///
/// Reads the configuration directly rather than the inventory. The inventory
/// probes every configured network printer, which costs seconds and opens a
/// connection to each — a startling thing for a warning about spelling to do,
/// and enough to consume a single-session printer's one accept.
fn warn_about_unresolvable_profiles(state: &super::ApiState) {
    let Ok(configuration) = crate::configuration::load(state.config.as_deref()) else {
        return;
    };
    for printer in configuration.printers() {
        if let Some(configured) = printer.profile()
            && super::printers::canonical_profile(Some(configured)).is_none()
        {
            eprintln!(
                "warning: printer {:?} has profile {configured:?}, which is not in the catalog; \
                 it will be reported as having none. Run `escpost profiles list` for valid ids.",
                printer.name()
            );
        }
    }
}
