//! The `telemetry` command: inspect or change the stored usage-data answer.
//! Storage itself lives in `crate::telemetry::consent`, shared with the
//! dispatcher's first-run prompt.

pub(crate) mod cli;
