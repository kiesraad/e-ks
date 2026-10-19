//! Composition root: the application state, the top-level route wiring, and the
//! HTTP middleware that injects state into requests.
#[cfg(all(test, fuzzing))]
mod access_rule_tests;
mod auth;
pub mod middleware;
pub mod router;
mod state;

pub use state::AppState;
