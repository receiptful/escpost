//! One HTTP POST per event, bounded in time, never reported to the user.
//!
//! The POST normally runs in a detached child process (`spawn_detached`),
//! because a cold HTTPS round trip to the EU ingress costs a distant user
//! more than a second, and the command must not wait for it.

use std::env;
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
/// user's terminal. On Windows, handle inheritance is disabled and no console
/// window is created.
pub(crate) fn spawn_detached(body: &str) -> std::io::Result<()> {
    let executable = env::current_exe()?;
    #[cfg(not(windows))]
    {
        use std::process::{Command, Stdio};

        Command::new(executable)
            .arg("__send-telemetry")
            .arg(body)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
    }
    #[cfg(windows)]
    {
        use std::io;

        use winsafe::{CreateProcess, STARTUPINFO, co};

        let executable = executable
            .to_str()
            .ok_or_else(|| io::Error::other("the telemetry helper path is not Unicode"))?;
        let command_line = format!(
            "{} __send-telemetry {}",
            quote_argument(executable),
            quote_argument(body),
        );
        // std::process on our supported Rust version inherits every inheritable
        // handle, including the parent's captured stdout/stderr, even with null
        // stdio. That holds the caller's pipes open until this helper exits.
        // CreateProcess with inheritance disabled avoids that delay. WinSafe owns
        // and closes the returned process/thread handles without waiting for exit.
        let _process = CreateProcess(
            Some(executable),
            Some(&command_line),
            None,
            None,
            false,
            co::CREATE::NO_WINDOW,
            &[],
            None,
            &mut STARTUPINFO::default(),
        )
        .map_err(io::Error::other)?;
    }
    Ok(())
}

/// Quote one argument for the Windows CRT parser, without invoking a shell.
#[cfg(windows)]
fn quote_argument(argument: &str) -> String {
    let mut quoted = String::from("\"");
    let mut backslashes = 0;
    for character in argument.chars() {
        if character == '\\' {
            backslashes += 1;
            continue;
        }
        let escapes = if character == '"' {
            backslashes * 2 + 1
        } else {
            backslashes
        };
        quoted.extend(std::iter::repeat_n('\\', escapes));
        quoted.push(character);
        backslashes = 0;
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}
