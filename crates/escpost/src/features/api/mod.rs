//! `escpost api`: the local REST surface the browser extension and local
//! backends call.
//!
//! Distinct from `features::capture`, which serves a virtual RAW TCP printer
//! and a preview viewer. This surface prints to real printers and renders
//! nothing.

pub(crate) mod cli;
mod error;
mod http;
mod origin;

/// What this build can do, advertised on `/info` so a client can detect a
/// daemon that predates a feature without a protocol version bump (D3).
const CAPABILITIES: &[&str] = &["usb", "tcp"];

/// Shared by every handler. Cheap to clone: axum clones it per request.
#[derive(Clone, Debug, Default)]
pub(crate) struct ApiState {
    /// When set, only this extension id may call. Absent by default, which
    /// accepts any extension — see `origin::origin_allowed`.
    pub(crate) extension_id: Option<String>,
}
