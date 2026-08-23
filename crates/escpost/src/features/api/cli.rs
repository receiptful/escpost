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

    super::http::serve(listener, super::ApiState::default())
        .await
        .map_err(CliError::ServeApi)
}
