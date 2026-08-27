use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::routing::{any, get};
use escpost_render::{
    CommandCode, CommandTrace, DecodedCommand, Effect, Justification, PaintLifecycle, StateChange,
};
use serde::Serialize;
use tokio::net::TcpListener;

pub(crate) mod error;
mod frontend;
mod job_store;
mod jobs;
mod status;

pub(crate) use job_store::JobStore;
use job_store::{JobRuntimeStatus, RenderedJob};

const FIRST_AUTOMATIC_PORT: u16 = 9000;
const LAST_AUTOMATIC_PORT: u16 = 9099;

#[derive(Clone)]
pub(crate) struct WebState {
    jobs: JobStore,
    status_metadata: status::ServerStatusMetadata,
    pub(crate) printer_monitor: crate::features::printers::monitor::PrinterMonitor,
}

/// Current wall-clock time in Unix epoch milliseconds, for job completion.
fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
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
    web_app: bool,
) -> std::io::Result<()> {
    let status_metadata = status::ServerStatusMetadata::resolve(virtual_printer_address);
    let mut router = Router::new()
        .merge(crate::features::printers::http::router())
        .merge(crate::features::profiles::http::router())
        .merge(status::route())
        .merge(jobs::router())
        .route("/health", get(health))
        .route("/api", any(error::not_found))
        .route("/api/{*path}", any(error::not_found));
    if web_app {
        router = router
            .route("/", get(frontend::index))
            .route("/assets/{*path}", get(frontend::asset))
            .route("/{*path}", get(frontend::index));
    }
    let router = router.with_state(WebState {
        jobs,
        status_metadata,
        printer_monitor: crate::features::printers::monitor::PrinterMonitor::new(None),
    });
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
}

/// Liveness check for containers and automated tests. Returns 200 while the
/// server is accepting requests, independent of whether any job was captured.
async fn health() -> &'static str {
    "ok"
}

async fn shutdown_signal() {
    // Failure to install a signal handler should stop the server rather than
    // leave a foreground developer command that cannot shut down cleanly.
    let _ = tokio::signal::ctrl_c().await;
}
