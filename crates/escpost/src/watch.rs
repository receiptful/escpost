use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use escpost_render::{RenderScale, TracedRenderResult};

use crate::application::{self, ApplicationError};
use crate::error::CliError;
use crate::features::rendering::{self, Request};
use crate::source::InputFormat;
use crate::{output, source, web};

const WATCH_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Clone)]
pub(crate) struct WatchConfig {
    pub(crate) source: PathBuf,
    pub(crate) format: InputFormat,
    pub(crate) profile: String,
    pub(crate) output: Option<PathBuf>,
    pub(crate) output_dir: Option<PathBuf>,
    pub(crate) sheet: Option<usize>,
    pub(crate) scale: RenderScale,
    pub(crate) antialias: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SourceStamp {
    modified: Option<SystemTime>,
    length: u64,
}

pub(crate) fn start(config: WatchConfig, jobs: web::JobStore) -> Result<(), CliError> {
    let watched_path = source_path(&config.source)?;
    let initial_stamp = inspect(&watched_path)?;
    tokio::spawn(run(config, watched_path, initial_stamp, jobs));
    Ok(())
}

pub(crate) fn source_path(path: &Path) -> Result<PathBuf, CliError> {
    if path == Path::new("-") {
        return Err(CliError::WatchStdin);
    }
    if path.is_dir() {
        return Ok(path.join("input.hex"));
    }
    Ok(path.to_owned())
}

async fn run(
    config: WatchConfig,
    watched_path: PathBuf,
    mut previous_stamp: SourceStamp,
    jobs: web::JobStore,
) {
    let mut interval = tokio::time::interval(WATCH_INTERVAL);
    loop {
        interval.tick().await;
        let current_stamp = match inspect(&watched_path) {
            Ok(stamp) => stamp,
            Err(error) => {
                jobs.set_error(error.to_string()).await;
                continue;
            }
        };
        if current_stamp == previous_stamp {
            continue;
        }
        previous_stamp = current_stamp;

        let render_config = config.clone();
        match tokio::task::spawn_blocking(move || rerender(&render_config)).await {
            Ok(Ok(rendered)) => jobs.replace_render(rendered).await,
            Ok(Err(error)) => jobs.set_error(error.to_string()).await,
            Err(error) => {
                jobs.set_error(format!("watched render task failed: {error}"))
                    .await;
            }
        }
    }
}

fn rerender(config: &WatchConfig) -> Result<TracedRenderResult, CliError> {
    let input = source::load(&config.source, config.format)?;
    let response = rendering::render(Request {
        bytes: input.bytes,
        profile_id: config.profile.clone(),
        scale: config.scale,
        antialias: config.antialias,
        trace: true,
    })?;
    if let Some(path) = &config.output {
        output::write_single(&response.render, path, config.sheet)?;
    }
    if let Some(directory) = &config.output_dir {
        output::write_all(&response.render, directory)?;
    }
    Ok(TracedRenderResult {
        render: response.render,
        trace: response.trace.expect("watch rendering requested a trace"),
    })
}

fn inspect(path: &PathBuf) -> application::Result<SourceStamp> {
    let metadata = fs::metadata(path).map_err(|source| ApplicationError::InspectWatchedSource {
        path: path.clone(),
        source,
    })?;
    Ok(SourceStamp {
        modified: metadata.modified().ok(),
        length: metadata.len(),
    })
}
