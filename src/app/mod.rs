//! Composition root: the application state, the top-level route wiring, and the
//! HTTP middleware that injects state into requests.
mod auth;
pub mod middleware;
pub mod router;
mod state;

pub use state::AppState;
