//! Application-layer boundaries shared by command features.

mod error;

pub(crate) use error::ApplicationError;

pub(crate) type Result<T> = std::result::Result<T, ApplicationError>;
