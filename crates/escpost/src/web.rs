use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use escpost_render::{
    CommandCode, CommandTrace, DecodedCommand, Effect, Justification, PaintLifecycle, StateChange,
    TracedRenderResult,
};
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::RwLock;

pub(crate) mod error;
mod frontend;
mod jobs;
mod status;

const INDEX_HTML: &str = include_str!("../assets/index.html");
const FIRST_AUTOMATIC_PORT: u16 = 9000;
const LAST_AUTOMATIC_PORT: u16 = 9099;

#[derive(Clone)]
pub(crate) struct JobStore {
    state: Arc<RwLock<JobStoreState>>,
}

#[derive(Clone)]
pub(crate) struct WebState {
    jobs: JobStore,
    virtual_printer_address: Option<SocketAddr>,
}

struct JobStoreState {
    jobs: VecDeque<Arc<RenderedJob>>,
    error: Option<String>,
    generation: u64,
    waiting_hint: Option<String>,
    completion: Option<&'static str>,
    receiving: usize,
    /// Profile the server renders with, shown before the first job arrives.
    session_profile: String,
    /// Whether renders are anti-aliased grayscale previews. The viewer smooths
    /// those and keeps the faithful 1-bit dots crisp.
    antialias: bool,
    /// Wall-clock time the current job completed, in Unix epoch milliseconds.
    completed_at: Option<u64>,
    /// The current job's exact captured bytes, offered for download.
    raw_input: Option<Vec<u8>>,
    /// Captured RAW jobs that successfully rendered during this server session.
    jobs_processed: u64,
}

pub(crate) struct JobRuntimeStatus {
    pub(crate) receiving: bool,
    pub(crate) jobs_processed: u64,
}

/// Current wall-clock time in Unix epoch milliseconds, for job completion.
fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

struct RenderedJob {
    profile: String,
    /// Non-fatal render diagnostics, pre-formatted for display.
    warnings: Vec<String>,
    sheets: Vec<RenderedWebSheet>,
    trace_sheets: Vec<TraceWebSheet>,
}

struct RenderedWebSheet {
    name: String,
    width_dots: u32,
    height_dots: u32,
    png: Vec<u8>,
}

struct TraceWebSheet {
    commands: Vec<CommandResponse>,
}

#[derive(Serialize)]
struct RenderResponse {
    profile: String,
    generation: u64,
    error: Option<String>,
    /// Guidance shown while no job has been captured yet, e.g. by `serve`.
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
    /// How a captured job ended: "closed" or "timeout". Absent for renders.
    #[serde(skip_serializing_if = "Option::is_none")]
    completion: Option<&'static str>,
    /// True while a connection is still sending a job that has not completed.
    receiving: bool,
    /// When the current job completed, in Unix epoch milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_at: Option<u64>,
    /// True when the current job's raw bytes can be downloaded from /job.
    input_available: bool,
    /// True when renders are anti-aliased grayscale, so the viewer smooths them.
    antialias: bool,
    /// Non-fatal render diagnostics for the current job, ready to display.
    warnings: Vec<String>,
    sheets: Vec<SheetResponse>,
}

#[derive(Serialize)]
struct SheetResponse {
    name: String,
    order: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    width_dots: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height_dots: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    /// Experimental subset of decoded commands associated with this sheet.
    commands: Vec<CommandResponse>,
}

#[derive(Clone, Serialize)]
struct CommandResponse {
    byte_start: usize,
    byte_end: usize,
    name: String,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    paint_lifecycle: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    annotation: Option<AnnotationResponse>,
    effects: Vec<EffectResponse>,
}

#[derive(Clone, Serialize)]
struct AnnotationResponse {
    label: String,
    content: String,
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum EffectResponse {
    StateChange {
        state: &'static str,
        before: &'static str,
        after: &'static str,
    },
    Motion {
        before: PositionResponse,
        after: PositionResponse,
    },
    Paint {
        bounds: RegionResponse,
    },
}

#[derive(Clone, Serialize)]
struct PositionResponse {
    x: u32,
    y: u32,
}

#[derive(Clone, Serialize)]
struct RegionResponse {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl JobStore {
    pub(crate) fn with_render(rendered: TracedRenderResult, antialias: bool) -> Self {
        Self {
            state: Arc::new(RwLock::new(JobStoreState {
                jobs: VecDeque::from([Arc::new(RenderedJob::from(rendered))]),
                error: None,
                generation: 1,
                waiting_hint: None,
                completion: None,
                receiving: 0,
                session_profile: String::new(),
                antialias,
                completed_at: Some(epoch_millis()),
                raw_input: None,
                jobs_processed: 0,
            })),
        }
    }

    /// Create a store with no job yet. The web viewer shows `hint` and the
    /// `profile` until the first job arrives, which suits a listener that
    /// renders on demand with a known profile.
    pub(crate) fn awaiting_jobs(profile: String, hint: String, antialias: bool) -> Self {
        Self {
            state: Arc::new(RwLock::new(JobStoreState {
                jobs: VecDeque::new(),
                error: None,
                generation: 0,
                waiting_hint: Some(hint),
                completion: None,
                receiving: 0,
                session_profile: profile,
                antialias,
                completed_at: None,
                raw_input: None,
                jobs_processed: 0,
            })),
        }
    }

    /// Mark that a connection has started sending a job. The viewer reports this
    /// until the matching `end_capture`.
    pub(crate) async fn begin_capture(&self) {
        self.state.write().await.receiving += 1;
    }

    /// Mark that an in-progress job has finished sending (completed or dropped).
    pub(crate) async fn end_capture(&self) {
        let mut state = self.state.write().await;
        state.receiving = state.receiving.saturating_sub(1);
    }

    /// Replace the preview with a render that has no capture semantics, such as
    /// `render --web`. Its source is a file, so nothing is offered for download.
    pub(crate) async fn replace_render(&self, rendered: TracedRenderResult) {
        self.store_render(rendered, None, None, false).await;
    }

    /// Replace the preview with a captured job, recording how it ended so the
    /// viewer can distinguish a closed connection from an idle timeout, and
    /// keeping its exact bytes for download.
    pub(crate) async fn replace_captured(
        &self,
        rendered: TracedRenderResult,
        completion: &'static str,
        raw_input: Vec<u8>,
    ) {
        self.store_render(rendered, Some(completion), Some(raw_input), true)
            .await;
    }

    async fn store_render(
        &self,
        rendered: TracedRenderResult,
        completion: Option<&'static str>,
        raw_input: Option<Vec<u8>>,
        captured: bool,
    ) {
        let mut state = self.state.write().await;
        state.jobs = VecDeque::from([Arc::new(RenderedJob::from(rendered))]);
        state.error = None;
        state.generation += 1;
        state.completion = completion;
        state.completed_at = Some(epoch_millis());
        state.raw_input = raw_input;
        if captured {
            state.jobs_processed += 1;
        }
    }

    pub(crate) async fn runtime_status(&self) -> JobRuntimeStatus {
        let state = self.state.read().await;
        JobRuntimeStatus {
            receiving: state.receiving > 0,
            jobs_processed: state.jobs_processed,
        }
    }

    /// The current job's captured bytes and completion time, for download.
    async fn raw_input_download(&self) -> Option<(Vec<u8>, u64)> {
        let state = self.state.read().await;
        let bytes = state.raw_input.clone()?;
        Some((bytes, state.completed_at.unwrap_or(0)))
    }

    pub(crate) async fn set_error(&self, error: String) {
        self.state.write().await.error = Some(error);
    }

    async fn snapshot(&self) -> Option<(Arc<RenderedJob>, u64, Option<String>)> {
        let state = self.state.read().await;
        state
            .jobs
            .front()
            .cloned()
            .map(|job| (job, state.generation, state.error.clone()))
    }

    async fn render_response(&self) -> RenderResponse {
        let state = self.state.read().await;
        let Some(job) = state.jobs.front() else {
            // No job yet: report a waiting state so the viewer can guide the
            // developer rather than showing a bare error.
            return RenderResponse {
                profile: state.session_profile.clone(),
                generation: state.generation,
                error: state.error.clone(),
                hint: state.waiting_hint.clone(),
                completion: None,
                receiving: state.receiving > 0,
                completed_at: None,
                input_available: false,
                antialias: state.antialias,
                warnings: Vec::new(),
                sheets: Vec::new(),
            };
        };
        let sheets = job
            .trace_sheets
            .iter()
            .enumerate()
            .map(|(index, trace_sheet)| {
                let rendered = job.sheets.get(index);
                SheetResponse {
                    name: rendered
                        .map(|sheet| sheet.name.clone())
                        .unwrap_or_else(|| format!("sheet-{:03}", index + 1)),
                    order: index + 1,
                    width_dots: rendered.map(|sheet| sheet.width_dots),
                    height_dots: rendered.map(|sheet| sheet.height_dots),
                    url: rendered.map(|_| format!("/sheets/{}.png", index + 1)),
                    commands: trace_sheet.commands.clone(),
                }
            })
            .collect();
        RenderResponse {
            profile: job.profile.clone(),
            generation: state.generation,
            error: state.error.clone(),
            hint: None,
            completion: state.completion,
            receiving: state.receiving > 0,
            completed_at: state.completed_at,
            input_available: state.raw_input.is_some(),
            antialias: state.antialias,
            warnings: job.warnings.clone(),
            sheets,
        }
    }
}

impl From<TracedRenderResult> for RenderedJob {
    fn from(traced: TracedRenderResult) -> Self {
        let TracedRenderResult { render, trace } = traced;
        Self {
            profile: render.metadata.profile_id,
            warnings: render.warnings.iter().map(ToString::to_string).collect(),
            sheets: render
                .sheets
                .into_iter()
                .enumerate()
                .map(|(index, sheet)| RenderedWebSheet {
                    name: format!("sheet-{:03}.png", index + 1),
                    width_dots: sheet.surface.width(),
                    height_dots: sheet.surface.height(),
                    png: sheet.png,
                })
                .collect(),
            trace_sheets: trace
                .sheets
                .into_iter()
                .map(|sheet| TraceWebSheet {
                    commands: command_responses(sheet.commands),
                })
                .collect(),
        }
    }
}

fn command_responses(commands: Vec<CommandTrace>) -> Vec<CommandResponse> {
    commands
        .into_iter()
        .map(|command| {
            let (name, detail, annotation) = match command.command {
                DecodedCommand::SetJustification(justification) => (
                    "ESC a".to_owned(),
                    format!("Set justification: {}", justification_name(justification)),
                    None,
                ),
                DecodedCommand::TextByte(byte) => (
                    "Text".to_owned(),
                    if byte.is_ascii_graphic() || byte == b' ' {
                        char::from(byte).to_string()
                    } else {
                        format!("0x{byte:02X}")
                    },
                    None,
                ),
                DecodedCommand::LineFeed => {
                    ("LF".to_owned(), "Print and line feed".to_owned(), None)
                }
                DecodedCommand::RasterImage => {
                    ("GS v 0".to_owned(), "Print raster image".to_owned(), None)
                }
                DecodedCommand::QrCode(data) => (
                    "GS ( k".to_owned(),
                    "Print QR code · Function 181".to_owned(),
                    Some(qr_annotation(&data)),
                ),
                DecodedCommand::Unmodeled(code) => {
                    let (name, detail) = unmodeled_command(code);
                    (name, detail, None)
                }
            };
            CommandResponse {
                byte_start: command.byte_range.start,
                byte_end: command.byte_range.end,
                name,
                detail,
                paint_lifecycle: command.paint_lifecycle.map(|lifecycle| match lifecycle {
                    PaintLifecycle::Buffered => "buffered",
                    PaintLifecycle::Committed => "committed",
                }),
                annotation,
                effects: command.effects.into_iter().map(effect_response).collect(),
            }
        })
        .collect()
}

fn unmodeled_command(code: CommandCode) -> (String, String) {
    let detail = "Parsed command · annotations not yet modeled".to_owned();
    match code {
        CommandCode::Control(0x09) => ("HT".to_owned(), detail),
        CommandCode::Control(0x0d) => ("CR".to_owned(), detail),
        CommandCode::Control(opcode) => (format!("Control {opcode:02X}"), detail),
        CommandCode::Esc(opcode) => (format!("ESC {}", opcode_name(opcode)), detail),
        CommandCode::Gs(opcode) => (format!("GS {}", opcode_name(opcode)), detail),
    }
}

fn opcode_name(opcode: u8) -> String {
    if opcode.is_ascii_graphic() {
        char::from(opcode).to_string()
    } else {
        format!("{opcode:02X}")
    }
}

fn qr_annotation(data: &[u8]) -> AnnotationResponse {
    match std::str::from_utf8(data) {
        Ok(text) => AnnotationResponse {
            label: text.chars().flat_map(char::escape_default).collect(),
            content: text.to_owned(),
        },
        Err(_) => {
            let hexadecimal = data
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            AnnotationResponse {
                label: hexadecimal.clone(),
                content: hexadecimal,
            }
        }
    }
}

fn effect_response(effect: Effect) -> EffectResponse {
    match effect {
        Effect::StateChange(StateChange::Justification { before, after }) => {
            EffectResponse::StateChange {
                state: "justification",
                before: justification_name(before),
                after: justification_name(after),
            }
        }
        Effect::Motion { before, after } => EffectResponse::Motion {
            before: PositionResponse {
                x: before.x,
                y: before.y,
            },
            after: PositionResponse {
                x: after.x,
                y: after.y,
            },
        },
        Effect::Paint { bounds } => EffectResponse::Paint {
            bounds: RegionResponse {
                x: bounds.x,
                y: bounds.y,
                width: bounds.width,
                height: bounds.height,
            },
        },
    }
}

fn justification_name(justification: Justification) -> &'static str {
    match justification {
        Justification::Left => "left",
        Justification::Center => "center",
        Justification::Right => "right",
    }
}

pub(crate) async fn bind(
    requested: Option<SocketAddr>,
) -> Result<TcpListener, crate::net::BindFailure> {
    crate::net::bind_loopback(requested, FIRST_AUTOMATIC_PORT..=LAST_AUTOMATIC_PORT).await
}

pub(crate) async fn serve(
    listener: TcpListener,
    jobs: JobStore,
    virtual_printer_address: Option<SocketAddr>,
) -> std::io::Result<()> {
    let router = Router::new()
        .merge(crate::features::printers::http::router())
        .merge(crate::features::profiles::http::router())
        .merge(status::route())
        .merge(jobs::router())
        .route("/", get(index))
        .route("/app", get(frontend::redirect))
        .route("/app/", get(frontend::index))
        .route("/app/assets/{*path}", get(frontend::asset))
        .route("/app/{*path}", get(frontend::index))
        .route("/health", get(health))
        .route("/api/render", get(current_render))
        .route("/api", any(error::not_found))
        .route("/api/{*path}", any(error::not_found))
        .route("/sheets/{file}", get(sheet_png))
        .route("/job", get(download_job))
        .with_state(WebState {
            jobs,
            virtual_printer_address,
        });
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

/// Liveness check for containers and automated tests. Returns 200 while the
/// server is accepting requests, independent of whether any job was captured.
async fn health() -> &'static str {
    "ok"
}

async fn current_render(State(state): State<WebState>) -> Json<RenderResponse> {
    Json(state.jobs.render_response().await)
}

async fn download_job(State(state): State<WebState>) -> Response {
    let Some((bytes, completed_at)) = state.jobs.raw_input_download().await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Name the file by completion time so several captures do not collide.
    let disposition = format!("attachment; filename=\"escpost-job-{completed_at}.bin\"");
    (
        [
            (
                header::CONTENT_TYPE,
                String::from("application/octet-stream"),
            ),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        bytes,
    )
        .into_response()
}

async fn sheet_png(Path(file): Path<String>, State(state): State<WebState>) -> Response {
    let Some(number) = file
        .strip_suffix(".png")
        .and_then(|number| number.parse::<usize>().ok())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((job, _, _)) = state.jobs.snapshot().await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(sheet) = number
        .checked_sub(1)
        .and_then(|index| job.sheets.get(index))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };

    ([(header::CONTENT_TYPE, "image/png")], sheet.png.clone()).into_response()
}

async fn shutdown_signal() {
    // Failure to install a signal handler should stop the server rather than
    // leave a foreground developer command that cannot shut down cleanly.
    let _ = tokio::signal::ctrl_c().await;
}
