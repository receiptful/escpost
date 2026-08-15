//! Application-layer boundaries shared by command features.

pub(crate) type Result<T> = std::result::Result<T, crate::error::CliError>;
