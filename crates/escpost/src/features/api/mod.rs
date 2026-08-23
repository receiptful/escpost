//! `escpost api`: the local REST surface the browser extension and local
//! backends call.
//!
//! Distinct from `features::capture`, which serves a virtual RAW TCP printer
//! and a preview viewer. This surface prints to real printers and renders
//! nothing.

pub(crate) mod cli;
mod http;

/// What this build can do, advertised on `/info` so a client can detect a
/// daemon that predates a feature without a protocol version bump (D3).
const CAPABILITIES: &[&str] = &["usb", "tcp"];
