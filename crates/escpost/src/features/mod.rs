//! User-facing application features.

/// Task 2 registers this contract in the live router. Until then it has no
/// production caller, but must remain available to that follow-up layer.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod api;
pub(crate) mod capture;
pub(crate) mod printers;
pub(crate) mod printing;
pub(crate) mod profiles;
pub(crate) mod rendering;
