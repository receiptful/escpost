//! Anonymous usage reporting: consent storage, event construction, delivery.
//!
//! The only caller is the root dispatcher in `lib.rs`. Feature modules never
//! touch this module, so what is reported is decided in one place.

pub(crate) mod consent;
