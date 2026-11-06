//! One HTTP POST per event, bounded in time, never reported to the user.
//!
//! The POST normally runs in a detached child process (`spawn_detached`),
//! because a cold HTTPS round trip to the EU ingress costs a distant user
//! more than a second, and the command must not wait for it.

use std::env;
use std::process::{Command, Stdio};
use std::time::Duration;

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

/// Post one already-serialised event body.
pub(crate) async fn send(endpoint: &str, body: &str) -> Result<(), reqwest::Error> {
    let client = reqwest::Client::builder().timeout(TIMEOUT).build()?;
    client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.to_owned())
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

/// Hand the body to a fresh `escpost __send-telemetry` process and return at
/// once. The child inherits the environment, so the endpoint override still
/// applies, and gets no stdio, so it can neither block on nor write to the
/// user's terminal. On Unix an orphaned child simply continues after the
/// parent exits; on Windows the flag keeps a console window from flashing.
pub(crate) fn spawn_detached(body: &str) -> std::io::Result<()> {
    let executable = env::current_exe()?;
    let mut command = Command::new(executable);
    command
        .arg("__send-telemetry")
        .arg(body)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.spawn()?;
    Ok(())
}
