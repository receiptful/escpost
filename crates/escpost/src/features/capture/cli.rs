//! Terminal adapter for RAW job capture and the embedded web viewer.

use std::io::IsTerminal;
use std::net::SocketAddr;
use std::time::Duration;

use clap::Args;
use escpost_profiles::PrinterProfile;
use escpost_render::RenderScale;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};

use crate::application::ApplicationError;
use crate::cli::web as cli_web;
use crate::error::CliError;
use crate::{net, profiles, web};

use super::{RenderRequest, render_job};

/// Port 9100 is the common RAW/AppSocket transport used by network printers. A
/// busy default escalates through this range, and every listener binds
/// loopback so captured receipt data is not exposed by default.
const FIRST_RAW_PORT: u16 = 9100;
const LAST_RAW_PORT: u16 = 9109;

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

    /// Preview pixel density: 1 to 3 subpixels per dot. 1 is dot resolution.
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

/// Decide whether to auto-open the web viewer in a browser. Open by default,
/// but stay out of the way when the user opted out (`--no-open`), when there is
/// no interactive terminal, or in automation (`--non-interactive`, `CI`, or
/// `BROWSER=none`).
fn should_open_browser(
    no_open: bool,
    non_interactive: bool,
    stderr_is_terminal: bool,
    browser_env: Option<&str>,
    ci: bool,
) -> bool {
    !no_open && !non_interactive && stderr_is_terminal && !ci && browser_env != Some("none")
}

pub(crate) async fn run(arguments: ServeArgs, non_interactive: bool) -> Result<(), CliError> {
    let scale = RenderScale::new(arguments.scale).map_err(ApplicationError::from)?;
    // Validate the configured profile before opening either listener. Captured
    // jobs pass that same validated profile to the synchronous rendering operation.
    let profile = profiles::load(&arguments.profile)?;
    eprintln!("Profile: {}", arguments.profile);

    // Zero disables the idle timeout; a negative or non-finite value is invalid.
    let idle_timeout = if arguments.idle_timeout == 0.0 {
        None
    } else if arguments.idle_timeout.is_finite() && arguments.idle_timeout > 0.0 {
        Some(Duration::from_secs_f64(arguments.idle_timeout))
    } else {
        return Err(CliError::InvalidIdleTimeout);
    };

    let raw = net::bind_loopback(arguments.listen, FIRST_RAW_PORT..=LAST_RAW_PORT)
        .await
        .map_err(|failure| match failure {
            net::BindFailure::Address { address, source } => {
                CliError::BindRawPrinter { address, source }
            }
            net::BindFailure::RangeExhausted => CliError::NoAutomaticRawPort,
        })?;
    let raw_address = raw.local_addr().map_err(CliError::ServeRawPrinter)?;
    if !raw_address.ip().is_loopback() {
        eprintln!("warning: the RAW printer accepts receipt data beyond loopback on {raw_address}");
    }
    eprintln!("RAW printer: {raw_address}");
    match idle_timeout {
        Some(timeout) => eprintln!("Idle timeout: {timeout:?}"),
        None => eprintln!("Idle timeout: disabled (jobs end when the connection closes)"),
    }

    let web_listener = cli_web::bind(arguments.web_listen).await?;
    let jobs = web::JobStore::awaiting_jobs(
        arguments.profile.clone(),
        format!(
            "Waiting for the first job. Configure a local ERP or POS application to send its RAW ESC/POS print jobs to {raw_address}."
        ),
        arguments.antialias,
    );

    // Accept jobs while the web viewer runs. The viewer owns the foreground and
    // returns on Ctrl+C; stop accepting once it does.
    let acceptor = tokio::spawn(accept_jobs(
        raw,
        jobs.clone(),
        profile,
        idle_timeout,
        scale,
        arguments.antialias,
    ));
    let open_browser = should_open_browser(
        arguments.no_open,
        non_interactive,
        std::io::stderr().is_terminal(),
        std::env::var("BROWSER").ok().as_deref(),
        std::env::var_os("CI").is_some(),
    );
    let result = cli_web::serve(web_listener, jobs, Some(raw_address), open_browser).await;
    acceptor.abort();
    result
}

async fn accept_jobs(
    listener: TcpListener,
    jobs: web::JobStore,
    profile: &'static PrinterProfile,
    idle_timeout: Option<Duration>,
    scale: RenderScale,
    antialias: bool,
) {
    loop {
        match listener.accept().await {
            // A transient accept error must not tear down the listener; the next
            // client can still connect.
            Ok((stream, _peer)) => {
                tokio::spawn(capture_job(
                    stream,
                    jobs.clone(),
                    profile,
                    idle_timeout,
                    scale,
                    antialias,
                ));
            }
            Err(_) => continue,
        }
    }
}

async fn capture_job(
    mut stream: TcpStream,
    jobs: web::JobStore,
    profile: &'static PrinterProfile,
    idle_timeout: Option<Duration>,
    scale: RenderScale,
    antialias: bool,
) {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    // Whether the viewer currently counts this connection as receiving a job.
    let mut receiving = false;
    loop {
        let read = match idle_timeout {
            Some(timeout) => match tokio::time::timeout(timeout, stream.read(&mut chunk)).await {
                Ok(result) => result,
                // Silence for the idle interval completes whatever has arrived.
                Err(_elapsed) => {
                    if !buffer.is_empty() {
                        finalize(
                            &jobs,
                            std::mem::take(&mut buffer),
                            profile,
                            "timeout",
                            scale,
                            antialias,
                        )
                        .await;
                        jobs.end_capture().await;
                        receiving = false;
                    }
                    continue;
                }
            },
            None => stream.read(&mut chunk).await,
        };
        match read {
            Ok(0) => break,
            Ok(read) => {
                buffer.extend_from_slice(&chunk[..read]);
                if !receiving {
                    jobs.begin_capture().await;
                    receiving = true;
                }
            }
            // A read error abandons whatever was buffered.
            Err(_) => {
                if receiving {
                    jobs.end_capture().await;
                }
                return;
            }
        }
    }
    // The connection closed: any remaining bytes are an explicitly completed job.
    if !buffer.is_empty() {
        finalize(&jobs, buffer, profile, "closed", scale, antialias).await;
    }
    if receiving {
        jobs.end_capture().await;
    }
}

async fn finalize(
    jobs: &web::JobStore,
    bytes: Vec<u8>,
    profile: &'static PrinterProfile,
    completion: &'static str,
    scale: RenderScale,
    antialias: bool,
) {
    // Rendering is synchronous and CPU-bound; run it off the async workers so a
    // job in flight cannot stall the web viewer's responses.
    match tokio::task::spawn_blocking(move || {
        render_job(RenderRequest {
            bytes,
            profile,
            scale,
            antialias,
        })
    })
    .await
    {
        Ok(Ok(response)) => {
            jobs.replace_captured(response.rendered, completion, response.raw_input)
                .await;
        }
        Ok(Err(error)) => {
            let message = match error {
                crate::application::ApplicationError::Render(source) => source.to_string(),
                error => error.to_string(),
            };
            eprintln!("warning: could not render captured job: {message}");
            jobs.set_error(message).await;
        }
        // A panic or cancellation in the render task leaves no job to preview.
        Err(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::should_open_browser;

    #[test]
    fn opens_by_default_on_an_interactive_terminal() {
        assert!(should_open_browser(false, false, true, None, false));
        // An explicit browser choice (not "none") still opens.
        assert!(should_open_browser(
            false,
            false,
            true,
            Some("firefox"),
            false
        ));
    }

    #[test]
    fn stays_out_of_the_way_when_opted_out_or_automated() {
        assert!(!should_open_browser(true, false, true, None, false)); // --no-open
        assert!(!should_open_browser(false, true, true, None, false)); // --non-interactive
        assert!(!should_open_browser(false, false, false, None, false)); // no terminal
        assert!(!should_open_browser(
            false,
            false,
            true,
            Some("none"),
            false
        )); // BROWSER=none
        assert!(!should_open_browser(false, false, true, None, true)); // CI
    }
}
