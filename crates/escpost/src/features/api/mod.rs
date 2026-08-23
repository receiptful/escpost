//! `escpost api`: the local REST surface the browser extension and local
//! backends call.
//!
//! Distinct from `features::capture`, which serves a virtual RAW TCP printer
//! and a preview viewer. This surface prints to real printers and renders
//! nothing.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;

pub(crate) mod cli;
mod error;
mod http;
mod origin;
mod print;
mod printers;

/// What this build can do, advertised on `/info` so a client can detect a
/// daemon that predates a feature without a protocol version bump (D3).
const CAPABILITIES: &[&str] = &["usb", "tcp", "device-identity"];

/// Shared by every handler. Cheap to clone: axum clones it per request.
#[derive(Clone, Debug, Default)]
pub(crate) struct ApiState {
    /// When set, only this extension id may call. Absent by default, which
    /// accepts any extension — see `origin::origin_allowed`.
    pub(crate) extension_id: Option<String>,
    /// Read printer configuration from this exact file rather than the
    /// default location.
    pub(crate) config: Option<std::path::PathBuf>,
    /// Names jobs within one run. Nothing persists a job id — the extension
    /// echoes it and discards it — so a counter is enough and a dependency on
    /// a UUID crate is not warranted.
    pub(crate) job_sequence: Arc<AtomicU64>,
}
