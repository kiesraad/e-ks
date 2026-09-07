//! Shared view layer: the Askama filters, the request-scoped template
//! context that both web sections render through, and the error response
//! whose page each section renders in its own layout.
mod context;
mod error_response;
pub mod filters;

pub use context::Context;
pub use error_response::ErrorPage;
// Only the `pg` and `csb` guard tests read these; see `context.rs`.
#[cfg(test)]
pub(crate) use context::{CSB_PAPER_CORRECTIONS_STOP_PREFIX, DOWNLOAD_WARNING_PREFIXES};
