//! One HTTP POST per event, bounded in time, never reported to the user.

use std::env;
use std::time::Duration;

use super::event::Event;

pub(crate) const DEFAULT_ENDPOINT: &str = "https://eu.i.posthog.com/i/v0/e/";
/// Testing hook: points the binary at a loopback listener. Not a user feature.
const ENDPOINT_ENV: &str = "ESCPOST_TELEMETRY_ENDPOINT";
/// The whole request, including DNS and TLS, must finish inside this budget.
/// A broken network may cost a command this much extra wall time and no more.
const TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) fn endpoint() -> String {
    env::var(ENDPOINT_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_owned())
}

pub(crate) async fn send(endpoint: &str, event: &Event) -> Result<(), reqwest::Error> {
    let client = reqwest::Client::builder().timeout(TIMEOUT).build()?;
    client
        .post(endpoint)
        .json(event)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
