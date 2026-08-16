//! Terminal adapter for the rendering operation.

use std::io::{self, IsTerminal, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum};
use escpost_render::{RenderScale, TracedRenderResult};
use inquire::Select;

use crate::application::ApplicationError;
use crate::error::CliError;
use crate::{output, profiles, source};

use super::{Request, render};

#[derive(Debug, Args)]
pub(crate) struct RenderArgs {
    /// Raw ESC/POS file, hexadecimal file, case directory, or - for stdin.
    pub(crate) source: PathBuf,

    /// Input representation.
    #[arg(long, value_enum, default_value_t = InputFormat::Auto)]
    format: InputFormat,

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

    /// Output pixel density: 1 to 3 subpixels per dot. 1 is dot resolution.
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) scale: u32,

    /// Anti-alias glyph edges into a grayscale preview (cosmetic; never what a
    /// printer emits). Pass --antialias for a nicer on-screen render.
    #[arg(long, num_args = 0..=1, default_value_t = false, default_missing_value = "true")]
    pub(crate) antialias: bool,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum InputFormat {
    #[default]
    Auto,
    Binary,
    Hex,
}

impl From<InputFormat> for source::InputFormat {
    fn from(format: InputFormat) -> Self {
        match format {
            InputFormat::Auto => Self::Auto,
            InputFormat::Binary => Self::Binary,
            InputFormat::Hex => Self::Hex,
        }
    }
}

pub(crate) async fn run(arguments: RenderArgs, non_interactive: bool) -> Result<(), CliError> {
    let web_enabled =
        arguments.web || arguments.browser || arguments.web_listen.is_some() || arguments.watch;
    let binary_stdout = arguments.output.as_deref() == Some(Path::new("-"));
    if arguments.output.is_none() && arguments.output_dir.is_none() && !web_enabled {
        return Err(CliError::MissingOutput);
    }
    if binary_stdout && web_enabled {
        return Err(CliError::StdoutWithWeb);
    }
    let scale = RenderScale::new(arguments.scale).map_err(ApplicationError::from)?;
    if arguments.watch {
        // Reject stdin before trying to consume it. A developer should get the
        // invalid-invocation error immediately, even if a producer never closes.
        crate::watch::source_path(&arguments.source)?;
    }

    let input = source::load(&arguments.source, arguments.format.into())?;
    let can_prompt = !non_interactive
        && !binary_stdout
        && arguments.source != Path::new("-")
        && io::stdin().is_terminal()
        && io::stderr().is_terminal();
    let requested_profile_id = resolve_profile(arguments.profile, input.profile, can_prompt)?;
    let response = render(Request {
        bytes: input.bytes,
        profile_id: requested_profile_id,
        scale,
        antialias: arguments.antialias,
        trace: web_enabled,
    })?;
    let profile_id = response.profile_id;
    let rendered = response.render;

    if !binary_stdout {
        eprintln!("Profile: {profile_id}");
    }
    // Non-fatal diagnostics (e.g. a cut dropped on a profile with no cutter) go
    // to stderr so they surface even when the rendered bytes are piped to stdout.
    for warning in &rendered.warnings {
        eprintln!("warning: {warning}");
    }

    if let Some(output_path) = &arguments.output {
        if binary_stdout {
            let png = output::single_png(&rendered, arguments.sheet)?;
            let mut stdout = io::stdout();
            write_stdout(png, stdout.is_terminal(), &mut stdout)?;
        } else {
            output::write_single(&rendered, output_path, arguments.sheet)?;
        }
    }
    if let Some(output_directory) = &arguments.output_dir {
        output::write_all(&rendered, output_directory)?;
    }
    if web_enabled {
        let listener = crate::cli::web::bind(arguments.web_listen).await?;
        let jobs = crate::web::JobStore::with_render(
            TracedRenderResult {
                render: rendered,
                trace: response.trace.expect("web rendering requested a trace"),
            },
            arguments.antialias,
        );
        if arguments.watch {
            crate::watch::start(
                crate::watch::WatchConfig {
                    source: arguments.source,
                    format: arguments.format.into(),
                    profile: profile_id,
                    output: arguments.output,
                    output_dir: arguments.output_dir,
                    sheet: arguments.sheet,
                    scale,
                    antialias: arguments.antialias,
                },
                jobs.clone(),
            )?;
        }
        crate::cli::web::serve(listener, jobs, arguments.browser).await?;
    }
    Ok(())
}

fn write_stdout(
    png: &[u8],
    stdout_is_terminal: bool,
    output: &mut impl Write,
) -> Result<(), CliError> {
    if stdout_is_terminal {
        return Err(CliError::BinaryOutputToTerminal);
    }
    output.write_all(png).map_err(CliError::WriteStdout)
}

fn resolve_profile(
    explicit: Option<String>,
    source_profile: Option<String>,
    can_prompt: bool,
) -> Result<String, CliError> {
    if let Some(profile) = explicit.or(source_profile) {
        return Ok(profile);
    }
    if !can_prompt {
        return Err(CliError::MissingProfile);
    }

    Select::new("Printer profile", profiles::available_ids()?)
        .with_help_message("Use REFERENCE when no physical printer is known")
        .prompt()
        .map_err(|error| CliError::ProfilePrompt(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_stdout_rejects_binary_png_bytes() {
        let mut output = Vec::new();

        let error = write_stdout(b"png", true, &mut output).expect_err("stdout should be rejected");

        assert!(matches!(error, CliError::BinaryOutputToTerminal));
        assert!(output.is_empty());
    }

    #[test]
    fn non_terminal_stdout_preserves_exact_png_bytes() {
        let mut output = Vec::new();

        write_stdout(b"\x89PNG\r\n", false, &mut output).expect("stdout should be writable");

        assert_eq!(output, b"\x89PNG\r\n");
    }
}
